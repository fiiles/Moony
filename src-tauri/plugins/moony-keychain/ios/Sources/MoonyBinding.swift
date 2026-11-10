// moony-keychain, iOS: device sync's binding to this phone, the macOS design ported
// (`macos/Sources/MoonyBiometrics/MoonyBiometrics.swift`, `moony_bind_seal` / `moony_bind_open`).
//
// The binding is sealed for a Secure Enclave P-256 key-agreement key whose access control is
// `.privateKeyUsage` alone (`kSecAttrAccessibleWhenUnlockedThisDeviceOnly`): no prompt, but only
// this phone's Secure Enclave can use it, and only while the phone is unlocked. The key never
// leaves the Secure Enclave; its `dataRepresentation` (an opaque handle only this Secure Enclave
// can use) travels inside the blob, so there is no keychain item, no entitlement, and nothing an
// app update loses. A blob from another phone fails to restore its key here.
//
// The seal is ECIES: an ephemeral P-256 key agrees with the Secure Enclave key's public half,
// HKDF-SHA256 (salted with both public keys, labelled "moony-binding-v1") turns the shared secret
// into an AES-256-GCM key, and the ephemeral public key travels in the blob. Byte for byte the
// macOS binding format:
//   0x42 | u16 BE length | SE key dataRepresentation | ephemeral public key x963 (65 B)
//        | AES.GCM combined (12 B nonce + 32 B ciphertext + 16 B tag)
//
// This file never decides that a blob belongs to another phone. A failure answers the stage it
// happened at (seal, parse, restore, agree, open) with the error's domain and code and those of
// the errors under it, and Rust classifies them with the macOS table (`models.rs`,
// `classify_ios_binding`; review PF11), so a refusal of the moment (the phone locked) can never
// read as "not this device". Without a Secure Enclave the answer is `unsupported`. (The Xcode 27
// iOS simulator reports `SecureEnclave.isAvailable` and runs this path; checked by
// `ios/Tests`.)
//
// Free functions only: the `@objc` commands that call them live in `MoonyKeychainPlugin.swift`
// (a static library drops Objective-C categories). The 32-byte value only ever lives in `Data`
// and byte arrays, wiped when this file is done with them. The base64 strings it crosses the
// bridge in (the argument of `bindSeal`, the answer of `bindOpen`) are Swift `String`s that cannot
// be wiped: the residual `lib.rs` accepts for the biometric secret too.

import CryptoKit
import Foundation
import Security

/// The first byte and the HKDF label of a binding blob (the macOS binding's).
private let bindingVersion: UInt8 = 0x42
private let bindingInfo = Data("moony-binding-v1".utf8)
private let bindingSecretLength = 32
private let bindingX963Length = 65
private let bindingSealedLength = 12 + 32 + 16

/// The domain of the failures this file finds itself. Rust reads it as `MOONY_BINDING_DOMAIN`.
private let moonyBindingDomain = "MoonyBinding"
private enum MoonyBindingCode: Int {
  /// The blob is not of the binding's format or length.
  case wrongFormat = 1
  /// A value that is not 32 bytes (to seal, or inside an opened box).
  case wrongSecretLength = 2
  /// AES-GCM gave no combined box.
  case noCombinedBox = 3
  /// The access control could not be created.
  case noAccessControl = 4
}

/// CryptoKit's errors cross as this domain with their case's index (`cryptoKitCode`).
private let cryptoKitDomain = "CryptoKit.CryptoKitError"
/// The value inside a CryptoKit `underlyingCoreCryptoError`, reported under it.
private let coreCryptoDomain = "CoreCrypto"
/// How many errors under the first one a failure reports (`NSUnderlyingErrorKey`).
private let underlyingLimit = 8

/// The step that failed. Rust's `BindStage` reads the raw values.
enum BindStage: String {
  case seal, parse, restore, agree, open
}

/// `bindSeal`'s and `bindOpen`'s answer (Rust: `models::bridge::BindReply`).
struct BindReply: Encodable {
  /// The blob (`bindSeal`) or the 32-byte value (`bindOpen`), as base64.
  let value: String?
  let unsupported: Bool
  let failure: BindFailureReport?

  static let unsupportedReply = BindReply(value: nil, unsupported: true, failure: nil)
}

struct BindFailureReport: Encodable {
  let stage: String
  let domain: String
  let code: Int
  let underlying: [BindErrorReport]
}

struct BindErrorReport: Encodable {
  let domain: String
  let code: Int
}

/// An error with the stage it happened at.
private struct StagedError: Error {
  let stage: BindStage
  let error: Error
}

/// The key a blob is sealed for: the Secure Enclave key in the app, a software key in
/// `bindingSelftest`. `keyData` is what goes into the blob to find the key again.
struct BindingRecipient {
  let keyData: Data
  let publicKey: P256.KeyAgreement.PublicKey
  let agree: (P256.KeyAgreement.PublicKey) throws -> SharedSecret
}

private struct ParsedBinding {
  let keyData: Data
  let ephemeralPublicKey: Data
  let sealed: Data
}

/// Seals the 32-byte `secret` for a new Secure Enclave key that needs no user presence. Answers
/// the blob, `unsupported` without a Secure Enclave, or the failure (stage `seal`).
func bindingSealReply(secret: Data) -> BindReply {
  var bytes = [UInt8](secret)
  defer { wipeBindingBytes(&bytes) }
  guard SecureEnclave.isAvailable else {
    return .unsupportedReply
  }
  do {
    guard bytes.count == bindingSecretLength else {
      throw moonyError(.wrongSecretLength)
    }
    var accessError: Unmanaged<CFError>?
    guard
      let access = SecAccessControlCreateWithFlags(
        kCFAllocatorDefault, kSecAttrAccessibleWhenUnlockedThisDeviceOnly, [.privateKeyUsage],
        &accessError)
    else {
      throw accessError.map { $0.takeRetainedValue() as Error } ?? moonyError(.noAccessControl)
    }
    let key = try SecureEnclave.P256.KeyAgreement.PrivateKey(accessControl: access)
    let recipient = BindingRecipient(
      keyData: key.dataRepresentation, publicKey: key.publicKey,
      agree: key.sharedSecretFromKeyAgreement)
    let blob = try sealBinding(bytes, for: recipient)
    return BindReply(value: blob.base64EncodedString(), unsupported: false, failure: nil)
  } catch {
    return bindingFailureReply(.seal, error)
  }
}

/// Opens a blob `bindingSealReply` made on this phone, without a prompt. Answers the 32-byte
/// value, `unsupported` without a Secure Enclave, or the stage that failed and its errors.
func bindingOpenReply(blob: Data) -> BindReply {
  guard SecureEnclave.isAvailable else {
    return .unsupportedReply
  }
  return bindingOpenReply(blob: blob, restore: secureEnclaveRecipient)
}

/// `bindingOpenReply` with the key found by `restore` (a software key in `bindingSelftest`).
func bindingOpenReply(blob: Data, restore: (Data) throws -> BindingRecipient) -> BindReply {
  do {
    var plaintext = try openBinding(blob, restore: restore)
    defer { wipeBindingData(&plaintext) }
    return BindReply(value: plaintext.base64EncodedString(), unsupported: false, failure: nil)
  } catch let failure as StagedError {
    return bindingFailureReply(failure.stage, failure.error)
  } catch {
    return bindingFailureReply(.open, error)
  }
}

/// The Secure Enclave key a binding blob carries the handle of.
private func secureEnclaveRecipient(_ keyData: Data) throws -> BindingRecipient {
  let key = try SecureEnclave.P256.KeyAgreement.PrivateKey(dataRepresentation: keyData)
  return BindingRecipient(
    keyData: keyData, publicKey: key.publicKey, agree: key.sharedSecretFromKeyAgreement)
}

/// The stage, domain and code of a failure, for the log: never a value, a blob or a key.
func describeBindingFailure(_ failure: BindFailureReport) -> String {
  let errors = ([BindErrorReport(domain: failure.domain, code: failure.code)] + failure.underlying)
    .map { "\($0.domain) \($0.code)" }
    .joined(separator: " < ")
  return "\(failure.stage): \(errors)"
}

// MARK: - ECIES

private func sealBinding(_ secret: [UInt8], for recipient: BindingRecipient) throws -> Data {
  let keyData = recipient.keyData
  let ephemeral = P256.KeyAgreement.PrivateKey()
  let ephemeralPublic = ephemeral.publicKey.x963Representation
  var plaintext = Data(secret)
  defer { wipeBindingData(&plaintext) }
  let shared = try ephemeral.sharedSecretFromKeyAgreement(with: recipient.publicKey)
  let key = bindingKey(
    shared, ephemeralPublic: ephemeralPublic,
    recipientPublic: recipient.publicKey.x963Representation)
  guard let sealed = try AES.GCM.seal(plaintext, using: key).combined else {
    throw moonyError(.noCombinedBox)
  }
  guard keyData.count <= Int(UInt16.max) else {
    throw moonyError(.wrongFormat)
  }
  var blob = Data([bindingVersion])
  blob.append(UInt8(keyData.count >> 8))
  blob.append(UInt8(keyData.count & 0xff))
  blob.append(keyData)
  blob.append(ephemeralPublic)
  blob.append(sealed)
  return blob
}

/// Parses the blob, restores its key with `restore`, agrees and decrypts; each step throws its
/// error with its stage.
private func openBinding(_ blob: Data, restore: (Data) throws -> BindingRecipient) throws -> Data
{
  let parsed = try staged(.parse) { try parseBinding(blob) }
  let ephemeralPublic = try staged(.parse) {
    try P256.KeyAgreement.PublicKey(x963Representation: parsed.ephemeralPublicKey)
  }
  let recipient = try staged(.restore) { try restore(parsed.keyData) }
  let shared = try staged(.agree) { try recipient.agree(ephemeralPublic) }
  let wrapping = bindingKey(
    shared, ephemeralPublic: parsed.ephemeralPublicKey,
    recipientPublic: recipient.publicKey.x963Representation)
  return try staged(.open) {
    let box = try AES.GCM.SealedBox(combined: parsed.sealed)
    var plaintext = try AES.GCM.open(box, using: wrapping)
    guard plaintext.count == bindingSecretLength else {
      wipeBindingData(&plaintext)
      throw moonyError(.wrongSecretLength)
    }
    return plaintext
  }
}

private func parseBinding(_ blob: Data) throws -> ParsedBinding {
  // `Data` slices keep their parent's indices; work on a copy that starts at zero.
  let bytes = Data(blob)
  guard bytes.count >= 3, bytes[0] == bindingVersion else {
    throw moonyError(.wrongFormat)
  }
  let keyLength = Int(bytes[1]) << 8 | Int(bytes[2])
  guard bytes.count == 3 + keyLength + bindingX963Length + bindingSealedLength else {
    throw moonyError(.wrongFormat)
  }
  let keyEnd = 3 + keyLength
  let ephemeralEnd = keyEnd + bindingX963Length
  return ParsedBinding(
    keyData: Data(bytes[3..<keyEnd]),
    ephemeralPublicKey: Data(bytes[keyEnd..<ephemeralEnd]),
    sealed: Data(bytes[ephemeralEnd...]))
}

/// HKDF-SHA256 over the shared secret, salted with both public keys, labelled with the format.
private func bindingKey(_ shared: SharedSecret, ephemeralPublic: Data, recipientPublic: Data)
  -> SymmetricKey
{
  return shared.hkdfDerivedSymmetricKey(
    using: SHA256.self, salt: ephemeralPublic + recipientPublic, sharedInfo: bindingInfo,
    outputByteCount: 32)
}

// MARK: - Failures

private func staged<T>(_ stage: BindStage, _ body: () throws -> T) throws -> T {
  do {
    return try body()
  } catch {
    throw StagedError(stage: stage, error: error)
  }
}

private func moonyError(_ code: MoonyBindingCode) -> NSError {
  return NSError(domain: moonyBindingDomain, code: code.rawValue)
}

/// The failure's stage, then the error and the ones under it (`NSUnderlyingErrorKey`), outermost
/// first, as domains and codes only: an error's description could quote a value it was given.
func bindingFailureReply(_ stage: BindStage, _ error: Error) -> BindReply {
  var reports = errorReports(error)
  var current = (error as NSError).userInfo[NSUnderlyingErrorKey] as? NSError
  while let next = current, reports.count <= underlyingLimit {
    reports += errorReports(next)
    current = next.userInfo[NSUnderlyingErrorKey] as? NSError
  }
  let first = reports.removeFirst()
  let failure = BindFailureReport(
    stage: stage.rawValue, domain: first.domain, code: first.code,
    underlying: Array(reports.prefix(underlyingLimit)))
  return BindReply(value: nil, unsupported: false, failure: failure)
}

/// An error as a domain and a code; a CryptoKit `underlyingCoreCryptoError` also gives its
/// CoreCrypto value, as an entry under it (domain `CoreCrypto`), so the log names the real error.
private func errorReports(_ error: Error) -> [BindErrorReport] {
  if let cryptoKitError = error as? CryptoKitError {
    var reports = [BindErrorReport(domain: cryptoKitDomain, code: cryptoKitCode(cryptoKitError))]
    if case .underlyingCoreCryptoError(let value) = cryptoKitError {
      reports.append(BindErrorReport(domain: coreCryptoDomain, code: Int(value)))
    }
    return reports
  }
  let failure = error as NSError
  return [BindErrorReport(domain: failure.domain, code: failure.code)]
}

/// A CryptoKit error's case, by its index in CryptoKit's declaration (`authenticationFailure`, a
/// box that does not open, is 2), spelled out rather than taken from the bridged `NSError`. The
/// cases iOS 13 lacks (`wrapFailure`, `unwrapFailure`, `invalidParameter`) answer 9: the package
/// still builds for iOS 13, and Rust reads every CryptoKit error alike.
private func cryptoKitCode(_ error: CryptoKitError) -> Int {
  switch error {
  case .incorrectKeySize: return 0
  case .incorrectParameterSize: return 1
  case .authenticationFailure: return 2
  case .underlyingCoreCryptoError: return 3
  default: return 9
  }
}

// MARK: - Selftest

/// The seal and open code above with a software P-256 key in place of the Secure Enclave key, so
/// the simulator can prove the blob format, the ECIES round trip and the stage of each failure
/// (`ios/Tests`, `xcodebuild test`), as `moony_bio_selftest` does on macOS. Answers the checks
/// that failed; empty when all pass.
func bindingSelftest() -> [String] {
  var failures: [String] = []
  func check(_ passed: Bool, _ what: String) {
    if !passed { failures.append(what) }
  }
  func failure(_ reply: BindReply) -> String {
    guard reply.value == nil, !reply.unsupported, let failure = reply.failure else {
      return "no failure"
    }
    return describeBindingFailure(failure)
  }

  let softwareKey = P256.KeyAgreement.PrivateKey()
  let recipient = BindingRecipient(
    keyData: softwareKey.rawRepresentation, publicKey: softwareKey.publicKey,
    agree: softwareKey.sharedSecretFromKeyAgreement)
  let restore: (Data) throws -> BindingRecipient = { keyData in
    let key = try P256.KeyAgreement.PrivateKey(rawRepresentation: keyData)
    return BindingRecipient(
      keyData: keyData, publicKey: key.publicKey, agree: key.sharedSecretFromKeyAgreement)
  }
  let secret = (0..<bindingSecretLength).map { UInt8($0) }

  let blob: Data
  do {
    blob = try sealBinding(secret, for: recipient)
  } catch {
    return ["seal threw \(bindingFailureReply(.seal, error).failure.map(describeBindingFailure) ?? "")"]
  }
  let keyLength = softwareKey.rawRepresentation.count
  check(blob.first == bindingVersion, "format byte 0x42")
  check(
    blob.count == 3 + keyLength + bindingX963Length + bindingSealedLength,
    "blob length")
  check(
    blob[1] == UInt8(keyLength >> 8) && blob[2] == UInt8(keyLength & 0xff), "key length prefix")
  check(
    !(0...(blob.count - secret.count)).contains { start in
      Array(blob[start..<(start + secret.count)]) == secret
    },
    "the secret is not in the blob")

  // Round trip, twice (a new ephemeral key each seal, the same open).
  let opened = bindingOpenReply(blob: blob, restore: restore)
  check(opened.failure == nil && !opened.unsupported, "open: \(failure(opened))")
  check(
    opened.value.flatMap { Data(base64Encoded: $0) } == Data(secret), "open gives the secret")

  // A flipped ciphertext bit: the box does not open (CryptoKit authenticationFailure).
  var tampered = blob
  tampered[tampered.count - 20] ^= 0x01
  check(
    failure(bindingOpenReply(blob: tampered, restore: restore))
      == "open: CryptoKit.CryptoKitError 2",
    "tampered: \(failure(bindingOpenReply(blob: tampered, restore: restore)))")

  // A cut blob and another format: parse, MoonyBinding wrongFormat.
  var relabelled = blob
  relabelled[0] = 0x01
  for (broken, name) in [(Data(blob.dropLast()), "cut"), (relabelled, "relabelled"), (Data(), "empty")] {
    let answer = failure(bindingOpenReply(blob: broken, restore: restore))
    check(answer == "parse: MoonyBinding 1", "\(name): \(answer)")
  }

  // A garbled ephemeral key (not a point on the curve): parse, a CryptoKit error.
  var garbled = blob
  garbled[3 + keyLength] = 0x05
  let garbledAnswer = failure(bindingOpenReply(blob: garbled, restore: restore))
  check(garbledAnswer.hasPrefix("parse: CryptoKit.CryptoKitError"), "garbled: \(garbledAnswer)")

  // A key handle this Secure Enclave rejects: restore, with the error as given.
  let rejected = failure(
    bindingOpenReply(blob: blob) { _ in throw NSError(domain: "CryptoTokenKit", code: -3) })
  check(rejected == "restore: CryptoTokenKit -3", "rejected handle: \(rejected)")

  // An agreement refused while the phone is locked: agree, -25308 kept under the outer error.
  let locked = NSError(
    domain: "CryptoTokenKit", code: -3,
    userInfo: [NSUnderlyingErrorKey: NSError(domain: NSOSStatusErrorDomain, code: -25308)])
  let refusing: (Data) throws -> BindingRecipient = { keyData in
    BindingRecipient(
      keyData: keyData, publicKey: softwareKey.publicKey, agree: { _ in throw locked })
  }
  let agreeAnswer = failure(bindingOpenReply(blob: blob, restore: refusing))
  check(
    agreeAnswer == "agree: CryptoTokenKit -3 < NSOSStatusErrorDomain -25308",
    "locked agreement: \(agreeAnswer)")

  // A wrong key: the box does not open.
  let otherKey = P256.KeyAgreement.PrivateKey()
  let wrongKey: (Data) throws -> BindingRecipient = { _ in
    BindingRecipient(
      keyData: otherKey.rawRepresentation, publicKey: otherKey.publicKey,
      agree: otherKey.sharedSecretFromKeyAgreement)
  }
  let wrongAnswer = failure(bindingOpenReply(blob: blob, restore: wrongKey))
  check(wrongAnswer == "open: CryptoKit.CryptoKitError 2", "wrong key: \(wrongAnswer)")

  return failures
}

// MARK: - Wiping

private func wipeBindingBytes(_ bytes: inout [UInt8]) {
  bytes.withUnsafeMutableBytes { buffer in
    guard let base = buffer.baseAddress else { return }
    _ = memset_s(base, buffer.count, 0, buffer.count)
  }
}

private func wipeBindingData(_ data: inout Data) {
  data.withUnsafeMutableBytes { buffer in
    guard let base = buffer.baseAddress else { return }
    _ = memset_s(base, buffer.count, 0, buffer.count)
  }
}
