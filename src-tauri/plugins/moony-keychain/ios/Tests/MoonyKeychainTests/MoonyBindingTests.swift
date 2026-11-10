// The binding's ECIES, blob format and failure reports, run on the simulator (no Secure Enclave
// there: `bindingSelftest` puts a software P-256 key in its place, as macOS's `moony_bio_selftest`
// does). Run with:
//   cd src-tauri/plugins/moony-keychain/ios
//   xcodebuild test -scheme moony-keychain -destination 'platform=iOS Simulator,name=iPhone 18 Pro'

import CryptoKit
import XCTest

@testable import moony_keychain

final class MoonyBindingTests: XCTestCase {
  /// Round trip, the 0x42 format, and every stage tagged: a cut or relabelled blob and a garbled
  /// ephemeral key at `parse`, a refused key handle at `restore`, a refused agreement at `agree`
  /// (a locked phone's -25308 kept underneath), a flipped ciphertext bit at `open` (CryptoKit
  /// authenticationFailure, 2).
  func testTheSelftestPasses() {
    XCTAssertEqual(bindingSelftest(), [])
  }

  /// CoreCrypto's own error value travels under the CryptoKit error, so the device log names it.
  func testACoreCryptoFailureKeepsItsValue() {
    let reply = bindingFailureReply(
      .restore, CryptoKitError.underlyingCoreCryptoError(error: -7))
    XCTAssertEqual(reply.failure?.stage, "restore")
    XCTAssertEqual(reply.failure?.domain, "CryptoKit.CryptoKitError")
    XCTAssertEqual(reply.failure?.code, 3)
    XCTAssertEqual(reply.failure?.underlying.map { "\($0.domain) \($0.code)" }, ["CoreCrypto -7"])
  }

  /// Where `SecureEnclave.isAvailable` (a device, and the simulator of recent Xcode releases):
  /// the real path seals and opens, and a cut blob is a parse failure.
  func testTheSecureEnclavePathRoundTrips() throws {
    try XCTSkipUnless(SecureEnclave.isAvailable, "no Secure Enclave here")
    let secret = Data((0..<32).map { UInt8($0) })
    let sealed = bindingSealReply(secret: secret)
    XCTAssertNil(sealed.failure.map(describeBindingFailure))
    let blob = try XCTUnwrap(sealed.value.flatMap { Data(base64Encoded: $0) })
    XCTAssertEqual(blob.first, 0x42)
    let opened = bindingOpenReply(blob: blob)
    XCTAssertNil(opened.failure.map(describeBindingFailure))
    XCTAssertEqual(opened.value.flatMap { Data(base64Encoded: $0) }, secret)
    let cut = bindingOpenReply(blob: blob.dropLast())
    XCTAssertEqual(cut.failure.map(describeBindingFailure), "parse: MoonyBinding 1")
  }

  /// Without a Secure Enclave both calls answer `unsupported`, nothing else.
  func testNoSecureEnclaveAnswersUnsupported() throws {
    try XCTSkipIf(SecureEnclave.isAvailable, "a device with a Secure Enclave")
    for reply in [
      bindingSealReply(secret: Data(repeating: 1, count: 32)),
      bindingOpenReply(blob: Data([0x42, 0, 0])),
    ] {
      XCTAssertTrue(reply.unsupported)
      XCTAssertNil(reply.value)
      XCTAssertNil(reply.failure)
    }
  }
}
