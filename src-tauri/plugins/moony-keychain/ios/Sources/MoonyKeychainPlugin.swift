// moony-keychain, iOS half.
//
// One secret per key in the Keychain, readable only after Face ID / Touch ID with the enrolment
// that existed when it was stored (`.biometryCurrentSet`), on this device only, and only while
// a passcode is set. Next to each secret sits a second, unprotected item with the biometric
// domain state of that enrolment, so `retrieve` can report "the enrolment changed" without
// prompting and without guessing from an OSStatus. `retrieve` prompts through LAContext and
// reads the item with that authenticated context (one prompt on a device, and a prompt at all
// on the simulator, whose keychain ignores biometric access control).
//
// Every @objc command lives in this file: the package links as a static library, and an
// extension in another file would be an Objective-C category the linker drops.
//
// Also the privacy screen: a blur over the key window while the app is not active, so the
// app-switcher snapshot shows no amounts.

import LocalAuthentication
import Security
import Tauri
import UIKit
import WebKit

private let keychainService = "com.moonyfinance.mobile.keychain"
private let domainStateSuffix = "/domain-state"
private let logCategory = "moony-keychain"

/// Rejection codes. The Rust side parses them with `KeychainError::from_rejection`.
enum KeychainCode: String {
  case invalidated
  case cancelled
  case unavailable
  case failed
}

struct StoreArgs: Decodable {
  let key: String
  let value: String
}

struct RetrieveArgs: Decodable {
  let key: String
  let reason: String
}

struct RemoveArgs: Decodable {
  let key: String
}

struct PrivacyScreenArgs: Decodable {
  let enabled: Bool
}

private enum ItemRead {
  case found(Data)
  case missing
  case error(OSStatus)
}

class MoonyKeychainPlugin: Plugin {
  /// Keychain calls block (`retrieve` until the person answers the prompt), so they run here and
  /// not on Tauri's serial "ipc" queue, which every plugin's commands share.
  private let workQueue = DispatchQueue(label: "com.moonyfinance.keychain", qos: .userInitiated)

  // Main thread only.
  private var privacyScreenEnabled = true
  private var privacyView: UIVisualEffectView?
  private var observers: [NSObjectProtocol] = []

  public override func load(webview: WKWebView) {
    guard observers.isEmpty else { return }
    let center = NotificationCenter.default
    observers.append(
      center.addObserver(
        forName: UIApplication.willResignActiveNotification, object: nil, queue: .main
      ) { [weak self] _ in
        self?.showPrivacyScreen()
      })
    observers.append(
      center.addObserver(
        forName: UIApplication.didBecomeActiveNotification, object: nil, queue: .main
      ) { [weak self] _ in
        self?.hidePrivacyScreen()
      })
  }

  deinit {
    for observer in observers {
      NotificationCenter.default.removeObserver(observer)
    }
  }

  // MARK: - Commands

  @objc func status(_ invoke: Invoke) {
    let context = LAContext()
    var policyError: NSError?
    var available = context.canEvaluatePolicy(
      .deviceOwnerAuthenticationWithBiometrics, error: &policyError)
    // Face ID without a usage description would terminate the app at the first prompt.
    if available && context.biometryType == .faceID && !hasFaceIDUsageDescription() {
      available = false
    }
    invoke.resolve([
      "available": available,
      "biometryType": biometryTypeName(context.biometryType),
    ])
  }

  @objc func store(_ invoke: Invoke) {
    guard let args = arguments(invoke, StoreArgs.self) else { return }
    workQueue.async { self.performStore(invoke, args) }
  }

  @objc func retrieve(_ invoke: Invoke) {
    guard let args = arguments(invoke, RetrieveArgs.self) else { return }
    workQueue.async { self.performRetrieve(invoke, args) }
  }

  @objc func remove(_ invoke: Invoke) {
    guard let args = arguments(invoke, RemoveArgs.self) else { return }
    workQueue.async {
      let secret = self.deleteItem(account: args.key)
      let state = self.deleteItem(account: args.key + domainStateSuffix)
      if let failure = [secret, state].first(where: {
        $0 != errSecSuccess && $0 != errSecItemNotFound
      }) {
        self.reject(invoke, .failed, "Could not remove the secret: \(self.describe(failure))")
      } else {
        invoke.resolve()
      }
    }
  }

  @objc func setPrivacyScreen(_ invoke: Invoke) {
    guard let args = arguments(invoke, PrivacyScreenArgs.self) else { return }
    // Answer at once: a Rust caller blocks on the answer, possibly on the main thread.
    DispatchQueue.main.async {
      self.privacyScreenEnabled = args.enabled
      if !args.enabled {
        self.hidePrivacyScreen()
      }
    }
    invoke.resolve()
  }

  // MARK: - Keychain

  private func performStore(_ invoke: Invoke, _ args: StoreArgs) {
    let context = LAContext()
    var policyError: NSError?
    guard
      context.canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: &policyError),
      let domainState = context.evaluatedPolicyDomainState
    else {
      reject(
        invoke, .unavailable,
        "Biometry is unavailable: \(policyError?.localizedDescription ?? "no enrolment state")")
      return
    }

    var accessError: Unmanaged<CFError>?
    guard
      let access = SecAccessControlCreateWithFlags(
        kCFAllocatorDefault, kSecAttrAccessibleWhenPasscodeSetThisDeviceOnly,
        .biometryCurrentSet, &accessError)
    else {
      let reason = accessError.map { String(describing: $0.takeRetainedValue()) } ?? "unknown"
      reject(invoke, .failed, "Could not create the access control: \(reason)")
      return
    }

    // Replace rather than update: the new item binds to today's enrolment.
    deleteItem(account: args.key)
    deleteItem(account: args.key + domainStateSuffix)

    var secretItem = itemQuery(account: args.key)
    secretItem[kSecValueData as String] = Data(args.value.utf8)
    secretItem[kSecAttrAccessControl as String] = access
    let secretStatus = SecItemAdd(secretItem as CFDictionary, nil)
    guard secretStatus == errSecSuccess else {
      reject(invoke, .failed, "Could not store the secret: \(describe(secretStatus))")
      return
    }

    // Readable without a prompt, and it outlives a removed passcode (which deletes the secret),
    // so `retrieve` reports that case as invalidated too.
    var stateItem = itemQuery(account: args.key + domainStateSuffix)
    stateItem[kSecValueData as String] = domainState
    stateItem[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
    let stateStatus = SecItemAdd(stateItem as CFDictionary, nil)
    guard stateStatus == errSecSuccess else {
      deleteItem(account: args.key)
      reject(invoke, .failed, "Could not store the enrolment state: \(describe(stateStatus))")
      return
    }

    invoke.resolve()
  }

  private func performRetrieve(_ invoke: Invoke, _ args: RetrieveArgs) {
    let storedState: Data
    switch readItem(account: args.key + domainStateSuffix) {
    case .found(let data):
      storedState = data
    case .missing:
      // Nothing stored under this key: answer without prompting.
      invoke.resolve(["value": NSNull()])
      return
    case .error(let status):
      reject(
        invoke, status == errSecInteractionNotAllowed ? .unavailable : .failed,
        "Could not read the enrolment state: \(describe(status))")
      return
    }

    let context = LAContext()
    var policyError: NSError?
    guard context.canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: &policyError)
    else {
      if enrolmentLost(policyError) {
        reject(invoke, .invalidated, "The biometric enrolment or the passcode was removed")
      } else {
        reject(
          invoke, .unavailable,
          "Biometry is unavailable: \(policyError?.localizedDescription ?? "unknown")")
      }
      return
    }
    guard context.evaluatedPolicyDomainState == storedState else {
      reject(invoke, .invalidated, "The biometric enrolment changed since the secret was stored")
      return
    }

    // Prompt explicitly, then read the item with the authenticated context: the keychain takes
    // an evaluated context without asking again, and the reply carries a precise LAError. (The
    // simulator's keychain does not enforce biometric access control by itself, so this is also
    // what makes it prompt there.)
    context.localizedReason = args.reason  // the non-deprecated kSecUseOperationPrompt
    context.localizedFallbackTitle = ""  // biometry only: no "Enter Password" button
    context.evaluatePolicy(
      .deviceOwnerAuthenticationWithBiometrics, localizedReason: args.reason
    ) { success, error in
      self.workQueue.async {
        guard success else {
          self.rejectEvaluation(invoke, error, storedState: storedState)
          return
        }
        guard context.evaluatedPolicyDomainState == storedState else {
          self.reject(
            invoke, .invalidated, "The biometric enrolment changed since the secret was stored")
          return
        }
        self.readSecret(invoke, account: args.key, context: context, storedState: storedState)
      }
    }
  }

  private func rejectEvaluation(_ invoke: Invoke, _ error: Error?, storedState: Data) {
    guard let laError = error as? LAError else {
      reject(invoke, .failed, "Biometric authentication failed: \(String(describing: error))")
      return
    }
    switch laError.code {
    case .userCancel, .systemCancel, .appCancel, .userFallback:
      reject(invoke, .cancelled, "Biometric authentication was cancelled")
    case .biometryNotEnrolled, .passcodeNotSet:
      reject(invoke, .invalidated, "The biometric enrolment or the passcode was removed")
    case .biometryLockout, .biometryNotAvailable, .notInteractive:
      reject(invoke, .unavailable, "Biometry is unavailable: \(laError.localizedDescription)")
    default:
      if enrolmentChanged(since: storedState) {
        reject(invoke, .invalidated, "The biometric enrolment changed since the secret was stored")
      } else {
        reject(invoke, .failed, "Biometric authentication failed: \(laError.localizedDescription)")
      }
    }
  }

  private func readSecret(
    _ invoke: Invoke, account: String, context: LAContext, storedState: Data
  ) {
    var query = itemQuery(account: account)
    query[kSecReturnData as String] = true
    query[kSecMatchLimit as String] = kSecMatchLimitOne
    query[kSecUseAuthenticationContext as String] = context
    var result: CFTypeRef?
    let status = SecItemCopyMatching(query as CFDictionary, &result)

    switch status {
    case errSecSuccess:
      guard let data = result as? Data, let value = String(data: data, encoding: .utf8) else {
        reject(invoke, .failed, "The stored secret is not UTF-8 text")
        return
      }
      invoke.resolve(["value": value])
    case errSecUserCanceled:
      reject(invoke, .cancelled, "Biometric authentication was cancelled")
    case errSecAuthFailed, errSecItemNotFound:
      // The enrolment may have changed between the check above and the prompt.
      if enrolmentChanged(since: storedState) {
        reject(invoke, .invalidated, "The biometric enrolment changed since the secret was stored")
      } else if status == errSecItemNotFound {
        invoke.resolve(["value": NSNull()])
      } else {
        reject(invoke, .failed, "Biometric authentication failed")
      }
    case errSecInteractionNotAllowed:
      reject(invoke, .unavailable, "The keychain cannot prompt right now")
    default:
      reject(invoke, .failed, "Could not read the secret: \(describe(status))")
    }
  }

  /// Every enrolment or the passcode is gone: `.biometryCurrentSet` items never come back.
  private func enrolmentLost(_ error: NSError?) -> Bool {
    guard let error = error, error.domain == LAErrorDomain else { return false }
    return error.code == LAError.Code.biometryNotEnrolled.rawValue
      || error.code == LAError.Code.passcodeNotSet.rawValue
  }

  private func enrolmentChanged(since storedState: Data) -> Bool {
    let context = LAContext()
    var policyError: NSError?
    guard context.canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: &policyError)
    else {
      return enrolmentLost(policyError)
    }
    return context.evaluatedPolicyDomainState != storedState
  }

  private func itemQuery(account: String) -> [String: Any] {
    return [
      kSecClass as String: kSecClassGenericPassword,
      kSecAttrService as String: keychainService,
      kSecAttrAccount as String: account,
    ]
  }

  /// Reads an item that needs no authentication (the domain state).
  private func readItem(account: String) -> ItemRead {
    var query = itemQuery(account: account)
    query[kSecReturnData as String] = true
    query[kSecMatchLimit as String] = kSecMatchLimitOne
    var result: CFTypeRef?
    let status = SecItemCopyMatching(query as CFDictionary, &result)
    switch status {
    case errSecSuccess:
      guard let data = result as? Data else { return .error(errSecDecode) }
      return .found(data)
    case errSecItemNotFound:
      return .missing
    default:
      return .error(status)
    }
  }

  /// Deleting never prompts, protected or not.
  @discardableResult
  private func deleteItem(account: String) -> OSStatus {
    return SecItemDelete(itemQuery(account: account) as CFDictionary)
  }

  private func describe(_ status: OSStatus) -> String {
    let message = SecCopyErrorMessageString(status, nil) as String? ?? "unknown"
    return "OSStatus \(status) (\(message))"
  }

  // MARK: - Privacy screen

  private func showPrivacyScreen() {
    guard privacyScreenEnabled, privacyView == nil, let window = hostWindow() else { return }
    let view = UIVisualEffectView(effect: UIBlurEffect(style: .systemThinMaterial))
    view.frame = window.bounds
    view.autoresizingMask = [.flexibleWidth, .flexibleHeight]
    view.accessibilityIdentifier = "moony-privacy-screen"
    window.addSubview(view)
    privacyView = view
    Logger.debug("privacy screen shown", category: logCategory)
  }

  private func hidePrivacyScreen() {
    guard let view = privacyView else { return }
    view.removeFromSuperview()
    privacyView = nil
    Logger.debug("privacy screen removed", category: logCategory)
  }

  private func hostWindow() -> UIWindow? {
    if let window = manager.viewController?.view.window {
      return window
    }
    return UIApplication.shared.connectedScenes
      .compactMap { $0 as? UIWindowScene }
      .flatMap { $0.windows }
      .first { $0.isKeyWindow }
  }

  // MARK: - Helpers

  private func arguments<T: Decodable>(_ invoke: Invoke, _ type: T.Type) -> T? {
    do {
      return try invoke.parseArgs(type)
    } catch {
      reject(invoke, .failed, "Invalid arguments: \(error)")
      return nil
    }
  }

  private func reject(_ invoke: Invoke, _ code: KeychainCode, _ message: String) {
    Logger.error("\(invoke.command) rejected (\(code.rawValue)): \(message)", category: logCategory)
    invoke.reject(message, code: code.rawValue)
  }

  private func hasFaceIDUsageDescription() -> Bool {
    let text = Bundle.main.object(forInfoDictionaryKey: "NSFaceIDUsageDescription") as? String
    return !(text ?? "").isEmpty
  }

  private func biometryTypeName(_ type: LABiometryType) -> String {
    switch type {
    case .faceID:
      return "faceId"
    case .touchID:
      return "touchId"
    case .none:
      return "none"
    default:
      // Optic ID and anything newer.
      return "biometric"
    }
  }
}

@_cdecl("init_plugin_moony_keychain")
public func initPlugin() -> Plugin {
  return MoonyKeychainPlugin()
}
