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
// read as "not this device". Without a Secure Enclave (the simulator) the answer is `unsupported`.
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
    let blob = try sealBinding(bytes, keyData: key.dataRepresentation, recipient: key.publicKey)
    return BindReply(value: blob.base64EncodedString(), unsupported: false, failure: nil)
  } catch {
    return failureReply(.seal, error)
  }
}

/// Opens a blob `bindingSealReply` made on this phone, without a prompt. Answers the 32-byte
/// value, `unsupported` without a Secure Enclave, or the stage that failed and its errors.
func bindingOpenReply(blob: Data) -> BindReply {
  guard SecureEnclave.isAvailable else {
    return .unsupportedReply
  }
  do {
    var plaintext = try openBinding(blob)
    defer { wipeBindingData(&plaintext) }
    return BindReply(value: plaintext.base64EncodedString(), unsupported: false, failure: nil)
  } catch let failure as StagedError {
    return failureReply(failure.stage, failure.error)
  } catch {
    return failureReply(.open, error)
  }
}

/// The stage, domain and code of a failure, for the log: never a value, a blob or a key.
func describeBindingFailure(_ failure: BindFailureReport) -> String {
  let errors = ([BindErrorReport(domain: failure.domain, code: failure.code)] + failure.underlying)
    .map { "\($0.domain) \($0.code)" }
    .joined(separator: " < ")
  return "\(failure.stage): \(errors)"
}

// MARK: - ECIES

private func sealBinding(
  _ secret: [UInt8], keyData: Data, recipient: P256.KeyAgreement.PublicKey
) throws -> Data {
  let ephemeral = P256.KeyAgreement.PrivateKey()
  let ephemeralPublic = ephemeral.publicKey.x963Representation
  var plaintext = Data(secret)
  defer { wipeBindingData(&plaintext) }
  let shared = try ephemeral.sharedSecretFromKeyAgreement(with: recipient)
  let key = bindingKey(
    shared, ephemeralPublic: ephemeralPublic, recipientPublic: recipient.x963Representation)
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

/// Parses the blob, restores the Secure Enclave key, agrees and decrypts; each step throws its
/// error with its stage.
private func openBinding(_ blob: Data) throws -> Data {
  let parsed = try staged(.parse) { try parseBinding(blob) }
  let ephemeralPublic = try staged(.parse) {
    try P256.KeyAgreement.PublicKey(x963Representation: parsed.ephemeralPublicKey)
  }
  let key = try staged(.restore) {
    try SecureEnclave.P256.KeyAgreement.PrivateKey(dataRepresentation: parsed.keyData)
  }
  let shared = try staged(.agree) { try key.sharedSecretFromKeyAgreement(with: ephemeralPublic) }
  let wrapping = bindingKey(
    shared, ephemeralPublic: parsed.ephemeralPublicKey,
    recipientPublic: key.publicKey.x963Representation)
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
private func failureReply(_ stage: BindStage, _ error: Error) -> BindReply {
  let first = errorReport(error)
  var underlying: [BindErrorReport] = []
  var current = (error as NSError).userInfo[NSUnderlyingErrorKey] as? NSError
  while let next = current, underlying.count < underlyingLimit {
    underlying.append(errorReport(next))
    current = next.userInfo[NSUnderlyingErrorKey] as? NSError
  }
  let failure = BindFailureReport(
    stage: stage.rawValue, domain: first.domain, code: first.code, underlying: underlying)
  return BindReply(value: nil, unsupported: false, failure: failure)
}

private func errorReport(_ error: Error) -> BindErrorReport {
  if let cryptoKitError = error as? CryptoKitError {
    return BindErrorReport(domain: cryptoKitDomain, code: cryptoKitCode(cryptoKitError))
  }
  let failure = error as NSError
  return BindErrorReport(domain: failure.domain, code: failure.code)
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
