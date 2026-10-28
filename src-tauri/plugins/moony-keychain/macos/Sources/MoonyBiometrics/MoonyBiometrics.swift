// moony-keychain, macOS half: Touch ID unlock with a Secure Enclave key.
//
// The master key is wrapped for a Secure Enclave P-256 key-agreement key whose access control is
// `[.privateKeyUsage, .biometryCurrentSet]`: only this Mac's Secure Enclave can use it, and only
// after a Touch ID match against the fingerprints enrolled when it was made. Adding or removing a
// fingerprint invalidates it for good. The key never leaves the Secure Enclave; what the app keeps
// is its `dataRepresentation` (an opaque handle only this Secure Enclave can use) inside the blob,
// so deleting the blob file deletes the key. No keychain item and no entitlement are involved.
//
// The wrap is ECIES: an ephemeral P-256 key agrees with the Secure Enclave key's public half (no
// prompt needed for that), HKDF-SHA256 turns the shared secret into an AES-256-GCM key, and the
// ephemeral public key travels in the blob. Unwrapping repeats the agreement on the Secure
// Enclave's side, which is the step the finger unlocks.
//
// Blob v1, built and parsed only here:
//   0x01 | u16 BE length | SE key dataRepresentation | ephemeral public key x963 (65 B)
//        | AES.GCM combined (12 B nonce + 32 B ciphertext + 16 B tag)
//
// Every exported result is an `SRData` whose first byte is a status code the Rust side decodes
// (`desktop/macos.rs`): 0 ok (the payload follows), 1 cancelled, 2 unavailable, 3 invalidated,
// 4 failed (a UTF-8 message follows). The secret only ever lives in `Data` and byte arrays, which
// are wiped when this file is done with them, never in a `String`, and no message quotes it. The
// `SRData` buffers that carry it across the bridge are swift-rs's and are freed, not wiped.
//
// The calls block until the person answers the prompt, so Rust makes them off the main thread.

import CryptoKit
import Foundation
import LocalAuthentication
import Security
import SwiftRs

private let blobVersion: UInt8 = 1
private let hkdfInfo = Data("moony-touchid-v1".utf8)
private let secretLength = 32
private let x963Length = 65
private let sealedLength = 12 + 32 + 16

private enum StatusCode: UInt8 {
  case ok = 0
  case cancelled = 1
  case unavailable = 2
  case invalidated = 3
  case failed = 4
}

private struct BiometricsError: Error {
  let code: StatusCode
  let message: String

  static func cancelled() -> BiometricsError {
    BiometricsError(code: .cancelled, message: "")
  }
}

/// The key the blob is wrapped for: the Secure Enclave key in the app, a software key in the
/// selftest. `keyData` is what goes into the blob to find the key again.
private struct Recipient {
  let keyData: Data
  let publicKey: P256.KeyAgreement.PublicKey
  let agree: (P256.KeyAgreement.PublicKey) throws -> SharedSecret
}

private struct ParsedBlob {
  let keyData: Data
  let ephemeralPublicKey: Data
  let sealed: Data
}

// MARK: - Exports

/// -1: no usable biometrics (no sensor, nothing enrolled, locked out, or no Secure Enclave);
/// 0: biometrics usable but of no type the app can name; 1: Touch ID (any other type on a Mac).
@_cdecl("moony_bio_status")
public func moonyBioStatus() -> Int {
  let context = LAContext()
  var policyError: NSError?
  guard SecureEnclave.isAvailable,
    context.canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: &policyError)
  else {
    return -1
  }
  return context.biometryType == .none ? 0 : 1
}

/// Confirms with Touch ID (the person switching biometric unlock on), then makes a new Secure
/// Enclave key and wraps the 32-byte secret for it. Payload: the blob.
@_cdecl("moony_bio_wrap")
public func moonyBioWrap(reason: SRString, secret: SRData) -> SRData {
  return reply {
    var bytes = secret.toArray()
    defer { wipe(&bytes) }
    guard bytes.count == secretLength else {
      throw BiometricsError(code: .failed, message: "The secret is not \(secretLength) bytes")
    }

    let context = biometricContext(reason: reason.toString())
    try authenticate(context)

    var accessError: Unmanaged<CFError>?
    guard
      let access = SecAccessControlCreateWithFlags(
        kCFAllocatorDefault, kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
        [.privateKeyUsage, .biometryCurrentSet], &accessError)
    else {
      let detail = accessError.map { describe($0.takeRetainedValue()) } ?? "unknown"
      throw BiometricsError(
        code: .failed, message: "Could not create the access control: \(detail)")
    }
    let key: SecureEnclave.P256.KeyAgreement.PrivateKey
    do {
      key = try SecureEnclave.P256.KeyAgreement.PrivateKey(
        accessControl: access, authenticationContext: context)
    } catch {
      throw BiometricsError(
        code: .failed, message: "Could not create the Secure Enclave key: \(describe(error))")
    }

    let recipient = Recipient(
      keyData: key.dataRepresentation, publicKey: key.publicKey,
      agree: key.sharedSecretFromKeyAgreement)
    return Array(try seal(bytes, for: recipient))
  }
}

/// Unwraps a blob `moony_bio_wrap` made, behind a Touch ID prompt showing `reason`. Payload: the
/// 32-byte secret.
@_cdecl("moony_bio_unwrap")
public func moonyBioUnwrap(reason: SRString, blob: SRData) -> SRData {
  return reply {
    let context = biometricContext(reason: reason.toString())
    return try open(
      Data(blob.toArray()),
      restore: { keyData in
        // Restoring needs no prompt; the key only asks for the finger when it is used, and then
        // takes the context authenticated below instead of asking again.
        let key = try SecureEnclave.P256.KeyAgreement.PrivateKey(
          dataRepresentation: keyData, authenticationContext: context)
        return Recipient(
          keyData: keyData, publicKey: key.publicKey, agree: key.sharedSecretFromKeyAgreement)
      },
      authorize: { try authenticate(context) })
  }
}

/// The wrap and unwrap code above with a software P-256 key in place of the Secure Enclave key, so
/// a test can prove the blob format and the ECIES round trip without a finger on the sensor.
@_cdecl("moony_bio_selftest")
public func moonyBioSelftest() -> Bool {
  let softwareKey = P256.KeyAgreement.PrivateKey()
  let restore: (Data) throws -> Recipient = { keyData in
    let key = try P256.KeyAgreement.PrivateKey(rawRepresentation: keyData)
    return Recipient(
      keyData: keyData, publicKey: key.publicKey, agree: key.sharedSecretFromKeyAgreement)
  }
  let secret = (0..<secretLength).map { UInt8($0) }
  do {
    let blob = try seal(
      secret,
      for: Recipient(
        keyData: softwareKey.rawRepresentation, publicKey: softwareKey.publicKey,
        agree: softwareKey.sharedSecretFromKeyAgreement))
    guard try open(blob, restore: restore, authorize: {}) == secret else { return false }

    // A flipped ciphertext bit and a cut blob must both be refused as invalidated.
    var tampered = blob
    tampered[tampered.count - 20] ^= 0x01
    for broken in [tampered, blob.prefix(blob.count - 1)] {
      do {
        _ = try open(Data(broken), restore: restore, authorize: {})
        return false
      } catch let error as BiometricsError where error.code == .invalidated {
        continue
      }
    }
    return true
  } catch {
    return false
  }
}

/// For the Rust tests: the status code `restoreFailure` (`stage` 0) or `agreementFailure` (`stage`
/// 1) gives an error in `domain` with `code`, bare or (`wrapped`) as the underlying error of an
/// unrelated outer error, the way CryptoKit and CryptoTokenKit wrap them.
@_cdecl("moony_bio_classify")
public func moonyBioClassify(stage: Int, domain: SRString, code: Int, wrapped: Bool) -> Int {
  var error = NSError(domain: domain.toString(), code: code)
  if wrapped {
    error = NSError(
      domain: "CryptoTokenKit", code: -3, userInfo: [NSUnderlyingErrorKey: error])
  }
  let failure = stage == 0 ? restoreFailure(error) : agreementFailure(error)
  return Int(failure.code.rawValue)
}

// MARK: - ECIES

private func seal(_ secret: [UInt8], for recipient: Recipient) throws -> Data {
  let ephemeral = P256.KeyAgreement.PrivateKey()
  let ephemeralPublic = ephemeral.publicKey.x963Representation
  let sealed: Data
  var plaintext = Data(secret)
  defer { wipe(&plaintext) }
  do {
    let shared = try ephemeral.sharedSecretFromKeyAgreement(with: recipient.publicKey)
    let key = wrappingKey(
      shared, ephemeralPublic: ephemeralPublic,
      recipientPublic: recipient.publicKey.x963Representation)
    guard let combined = try AES.GCM.seal(plaintext, using: key).combined else {
      throw BiometricsError(code: .failed, message: "AES-GCM gave no combined box")
    }
    sealed = combined
  } catch let error as BiometricsError {
    throw error
  } catch {
    throw BiometricsError(code: .failed, message: "Could not wrap the secret: \(describe(error))")
  }

  guard recipient.keyData.count <= Int(UInt16.max) else {
    throw BiometricsError(code: .failed, message: "The key handle is too long for the blob")
  }
  var blob = Data([blobVersion])
  blob.append(UInt8(recipient.keyData.count >> 8))
  blob.append(UInt8(recipient.keyData.count & 0xff))
  blob.append(recipient.keyData)
  blob.append(ephemeralPublic)
  blob.append(sealed)
  return blob
}

/// Parses the blob, finds the key again, lets `authorize` prompt, then agrees and decrypts. A blob
/// that does not parse, a key that is gone, an agreement the key refuses and a box that does not
/// open are all "invalidated": the blob can never be read again.
private func open(
  _ blob: Data, restore: (Data) throws -> Recipient, authorize: () throws -> Void
) throws -> [UInt8] {
  let parsed = try parse(blob)
  let recipient: Recipient
  let ephemeralPublic: P256.KeyAgreement.PublicKey
  do {
    ephemeralPublic = try P256.KeyAgreement.PublicKey(x963Representation: parsed.ephemeralPublicKey)
  } catch {
    throw BiometricsError(code: .invalidated, message: "The blob's ephemeral key is malformed")
  }
  do {
    recipient = try restore(parsed.keyData)
  } catch {
    throw restoreFailure(error)
  }

  try authorize()

  let shared: SharedSecret
  do {
    shared = try recipient.agree(ephemeralPublic)
  } catch {
    throw agreementFailure(error)
  }

  let key = wrappingKey(
    shared, ephemeralPublic: parsed.ephemeralPublicKey,
    recipientPublic: recipient.publicKey.x963Representation)
  var plaintext: Data
  do {
    plaintext = try AES.GCM.open(AES.GCM.SealedBox(combined: parsed.sealed), using: key)
  } catch {
    throw BiometricsError(code: .invalidated, message: "The blob does not open")
  }
  defer { wipe(&plaintext) }
  guard plaintext.count == secretLength else {
    throw BiometricsError(
      code: .invalidated, message: "The blob holds no \(secretLength)-byte secret")
  }
  return Array(plaintext)
}

private func parse(_ blob: Data) throws -> ParsedBlob {
  // `Data` slices keep their parent's indices; work on a copy that starts at zero.
  let bytes = Data(blob)
  guard bytes.count >= 3, bytes[0] == blobVersion else {
    throw BiometricsError(code: .invalidated, message: "The blob is not a version 1 Touch ID blob")
  }
  let keyLength = Int(bytes[1]) << 8 | Int(bytes[2])
  guard bytes.count == 3 + keyLength + x963Length + sealedLength else {
    throw BiometricsError(code: .invalidated, message: "The blob has the wrong length")
  }
  let keyEnd = 3 + keyLength
  let ephemeralEnd = keyEnd + x963Length
  return ParsedBlob(
    keyData: Data(bytes[3..<keyEnd]),
    ephemeralPublicKey: Data(bytes[keyEnd..<ephemeralEnd]),
    sealed: Data(bytes[ephemeralEnd...]))
}

/// HKDF-SHA256 over the shared secret, salted with both public keys so the AES key is bound to this
/// exact pair, and labelled with the format version.
private func wrappingKey(_ shared: SharedSecret, ephemeralPublic: Data, recipientPublic: Data)
  -> SymmetricKey
{
  return shared.hkdfDerivedSymmetricKey(
    using: SHA256.self, salt: ephemeralPublic + recipientPublic, sharedInfo: hkdfInfo,
    outputByteCount: 32)
}

// MARK: - Touch ID

/// Biometry only: no "Enter Password" button, so the Mac login password never releases the key.
/// LAContext refuses an empty reason, hence the app's name in its place.
private func biometricContext(reason: String) -> LAContext {
  let context = LAContext()
  context.localizedReason = reason.isEmpty ? "Moony" : reason
  context.localizedFallbackTitle = ""
  return context
}

/// Shows the Touch ID prompt and blocks until the person answers it. The reply comes on a private
/// queue, so waiting here never deadlocks, whichever thread the caller is on (not the main one).
private func authenticate(_ context: LAContext) throws {
  let answered = DispatchSemaphore(value: 0)
  let outcome = PromptOutcome()
  context.evaluatePolicy(
    .deviceOwnerAuthenticationWithBiometrics, localizedReason: context.localizedReason
  ) { success, error in
    if !success {
      outcome.failure =
        error ?? BiometricsError(code: .failed, message: "Touch ID failed without a reason")
    }
    answered.signal()
  }
  answered.wait()
  if let failure = outcome.failure {
    throw classify(failure)
  }
}

/// The prompt's answer, written on LocalAuthentication's queue and read after the semaphore, which
/// orders the two.
private final class PromptOutcome: @unchecked Sendable {
  var failure: Error?
}

private func classify(_ error: Error) -> BiometricsError {
  if let error = error as? BiometricsError {
    return error
  }
  guard let laError = error as? LAError else {
    return BiometricsError(code: .failed, message: "Touch ID failed: \(describe(error))")
  }
  switch laError.code {
  case .userCancel, .appCancel, .systemCancel, .userFallback, .notInteractive:
    // `.notInteractive`: the prompt came while the app could not show it; the person tries again.
    return .cancelled()
  case .biometryNotAvailable, .biometryNotEnrolled, .biometryLockout, .passcodeNotSet:
    return BiometricsError(
      code: .unavailable, message: "Touch ID is unavailable: \(laError.localizedDescription)")
  case .authenticationFailed:
    return BiometricsError(
      code: .failed, message: "Touch ID did not recognise the fingerprint")
  default:
    return BiometricsError(
      code: .failed, message: "Touch ID failed: \(laError.localizedDescription)")
  }
}

/// The Secure Enclave key could not be restored from its handle (before any prompt). Invalidated
/// only when the handle itself is unusable; a refusal of the moment (no interaction allowed in
/// this session, the Secure Enclave not reachable, a missing entitlement) is a failure, so the app
/// keeps biometric unlock on and the next attempt can work.
private func restoreFailure(_ error: Error) -> BiometricsError {
  if let laError = laError(in: error) {
    return classify(laError)
  }
  let transient: Set<Int> = [
    Int(errSecInteractionNotAllowed), Int(errSecNotAvailable), Int(errSecMissingEntitlement),
  ]
  if let status = osStatus(in: error), transient.contains(status) {
    return BiometricsError(
      code: .failed, message: "The Touch ID key cannot be used right now: \(describe(error))")
  }
  if osStatus(in: error) == Int(errSecUserCanceled) {
    return .cancelled()
  }
  return BiometricsError(
    code: .invalidated, message: "The Touch ID key is gone: \(describe(error))")
}

/// The key agreement failed after a successful prompt. If the Secure Enclave asked again, its
/// LocalAuthentication answer (a cancel, a misread finger, a lockout) is classified like the first
/// prompt's, so none of them switches biometric unlock off. Anything else means the key refused an
/// authenticated context: the enrolment it was bound to is gone.
private func agreementFailure(_ error: Error) -> BiometricsError {
  if let laError = laError(in: error) {
    return classify(laError)
  }
  if osStatus(in: error) == Int(errSecUserCanceled) {
    return .cancelled()
  }
  return BiometricsError(
    code: .invalidated, message: "The Touch ID key refused the unlock: \(describe(error))")
}

/// The first LocalAuthentication error in `error` or its underlying errors (CryptoKit and
/// CryptoTokenKit wrap them).
private func laError(in error: Error) -> LAError? {
  return firstError(in: error) { $0.domain == LAErrorDomain }.flatMap { found in
    LAError.Code(rawValue: found.code).map { LAError($0, userInfo: found.userInfo) }
  }
}

/// The first OSStatus in `error` or its underlying errors.
private func osStatus(in error: Error) -> Int? {
  return firstError(in: error) { $0.domain == NSOSStatusErrorDomain }?.code
}

private func firstError(in error: Error, where matches: (NSError) -> Bool) -> NSError? {
  var current: NSError? = error as NSError
  while let candidate = current {
    if matches(candidate) {
      return candidate
    }
    current = candidate.userInfo[NSUnderlyingErrorKey] as? NSError
  }
  return nil
}

// MARK: - Helpers

/// Runs `body` and frames its answer for Rust: the status byte, then the payload or the message.
/// The payload array is wiped once it is copied into the reply.
private func reply(_ body: () throws -> [UInt8]) -> SRData {
  do {
    var payload = try body()
    defer { wipe(&payload) }
    var framed = [UInt8]()
    framed.reserveCapacity(1 + payload.count)
    framed.append(StatusCode.ok.rawValue)
    framed.append(contentsOf: payload)
    return SRData(framed)
  } catch {
    let failure = classify(error)
    return SRData([failure.code.rawValue] + Array(failure.message.utf8))
  }
}

/// Domain and code only: an error's description could in principle quote a value it was given.
private func describe(_ error: Error) -> String {
  let failure = error as NSError
  return "\(failure.domain) \(failure.code)"
}

private func wipe(_ bytes: inout [UInt8]) {
  bytes.withUnsafeMutableBytes { buffer in
    guard let base = buffer.baseAddress else { return }
    _ = memset_s(base, buffer.count, 0, buffer.count)
  }
}

private func wipe(_ data: inout Data) {
  data.withUnsafeMutableBytes { buffer in
    guard let base = buffer.baseAddress else { return }
    _ = memset_s(base, buffer.count, 0, buffer.count)
  }
}
