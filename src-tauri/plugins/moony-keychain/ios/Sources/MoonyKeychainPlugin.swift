// moony-keychain, iOS half.
//
// One secret per key in the Keychain, readable only after Face ID / Touch ID with the enrolment
// that existed when it was stored (`.biometryCurrentSet`), on this device only, and only while
// a passcode is set. Next to each secret sits a second, unprotected item with the biometric
// domain state of that enrolment, so `retrieve` can report "the enrolment changed" without
// prompting and without guessing from an OSStatus. `store` and `retrieve` both prompt through
// LAContext and use that authenticated context for the keychain call: `store` so the person
// confirms the switch with their face or finger (and the system's Face ID permission alert comes
// in that context, not at the next lock), `retrieve` for one prompt on a device, and a prompt at
// all on the simulator, whose keychain ignores biometric access control.
//
// Every @objc command lives in this file: the package links as a static library, and an
// extension in another file would be an Objective-C category the linker drops.
//
// Also the privacy screen: a blur over the key window while the app is not active, so the
// app-switcher snapshot shows no amounts.
//
// And the status bar style: the app's theme, set outright through `preferredStatusBarStyle` of
// Tauri's view controller, because the sampled style lags behind a live theme switch on iOS 26
// and does not exist before iOS 26 (see `setStatusBarStyle`).

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
  /// The text of the Face ID / Touch ID prompt shown before the item is added ("Zapnout
  /// odemykání biometrií"); the app's name when missing or empty.
  let reason: String?
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

struct StatusBarStyleArgs: Decodable {
  /// The app's resolved theme: "light" or "dark".
  let theme: String
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
  // Main thread only: the theme the status bar follows and the view controller class whose
  // `preferredStatusBarStyle` already reads it.
  private var statusBarTheme: String?
  private var statusBarHookClass: ObjectIdentifier?

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

  @objc func setStatusBarStyle(_ invoke: Invoke) {
    guard let args = arguments(invoke, StatusBarStyleArgs.self) else { return }
    // Answer at once, as `setPrivacyScreen` does; UIKit is main-thread only.
    DispatchQueue.main.async {
      self.statusBarTheme = args.theme
      self.applyStatusBarStyle()
    }
    invoke.resolve()
  }

  // MARK: - Keychain

  private func performStore(_ invoke: Invoke, _ args: StoreArgs) {
    let context = LAContext()
    var policyError: NSError?
    guard context.canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: &policyError)
    else {
      reject(
        invoke, .unavailable,
        "Biometry is unavailable: \(policyError?.localizedDescription ?? "unknown")")
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

    // Replace rather than update: the new item binds to today's enrolment. Removed before the
    // prompt, so a refused prompt leaves nothing behind either (the app then records "off").
    deleteItem(account: args.key)
    deleteItem(account: args.key + domainStateSuffix)

    // Ask first, the same way `retrieve` does: the person confirms the switch, and a first
    // Face ID use brings the system's permission alert here, in context, not at the next lock.
    let reason = promptReason(args.reason)
    context.localizedReason = reason  // the non-deprecated kSecUseOperationPrompt
    context.localizedFallbackTitle = ""  // biometry only: no "Enter Password" button
    context.evaluatePolicy(
      .deviceOwnerAuthenticationWithBiometrics, localizedReason: reason
    ) { success, error in
      self.workQueue.async {
        guard success else {
          self.rejectStoreEvaluation(invoke, error)
          return
        }
        // The enrolment that has just answered.
        guard let domainState = context.evaluatedPolicyDomainState else {
          self.reject(invoke, .unavailable, "Biometry is unavailable: no enrolment state")
          return
        }
        self.addSecret(invoke, args, access: access, context: context, domainState: domainState)
      }
    }
  }

  /// Adds the secret with the context the person has just authenticated, then its domain state.
  private func addSecret(
    _ invoke: Invoke, _ args: StoreArgs, access: SecAccessControl, context: LAContext,
    domainState: Data
  ) {
    var secretItem = itemQuery(account: args.key)
    secretItem[kSecValueData as String] = Data(args.value.utf8)
    secretItem[kSecAttrAccessControl as String] = access
    secretItem[kSecUseAuthenticationContext as String] = context
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
        reject(invoke, .invalidated, "The enrolment, the passcode or the permission is gone")
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
    case .userCancel, .systemCancel, .appCancel, .userFallback, .notInteractive:
      // `.notInteractive`: the prompt came before the app was active; the person taps the button.
      reject(invoke, .cancelled, "Biometric authentication was cancelled")
    case .biometryNotEnrolled, .passcodeNotSet, .biometryNotAvailable:
      // `.biometryNotAvailable`: the Face ID permission was taken back (or there is no sensor),
      // so the secret is out of reach for good; the app switches biometric unlock off instead
      // of failing at every lock.
      reject(invoke, .invalidated, "The enrolment, the passcode or the permission is gone")
    case .biometryLockout:
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

  /// A refused prompt while switching on: nothing was stored (the app records "off").
  private func rejectStoreEvaluation(_ invoke: Invoke, _ error: Error?) {
    guard let laError = error as? LAError else {
      reject(invoke, .failed, "Biometric authentication failed: \(String(describing: error))")
      return
    }
    switch laError.code {
    case .userCancel, .systemCancel, .appCancel, .userFallback, .notInteractive:
      reject(invoke, .cancelled, "Biometric authentication was cancelled")
    case .biometryNotAvailable, .biometryNotEnrolled, .passcodeNotSet:
      // `.biometryNotAvailable` includes "Don't Allow" on the system's Face ID permission alert.
      reject(invoke, .unavailable, "Biometry is unavailable: \(laError.localizedDescription)")
    default:
      // `.biometryLockout`, `.authenticationFailed` and anything newer.
      reject(invoke, .failed, "Biometric authentication failed: \(laError.localizedDescription)")
    }
  }

  /// Every enrolment, the passcode or the app's permission to use biometry is gone (no sensor
  /// counts too): `.biometryCurrentSet` items are out of reach for good. Used on `retrieve` only.
  private func enrolmentLost(_ error: NSError?) -> Bool {
    guard let error = error, error.domain == LAErrorDomain else { return false }
    return error.code == LAError.Code.biometryNotEnrolled.rawValue
      || error.code == LAError.Code.passcodeNotSet.rawValue
      || error.code == LAError.Code.biometryNotAvailable.rawValue
  }

  /// The prompt text, else the app's name (LAContext refuses an empty reason).
  private func promptReason(_ reason: String?) -> String {
    if let reason = reason, !reason.isEmpty {
      return reason
    }
    let info = Bundle.main.infoDictionary
    return info?["CFBundleDisplayName"] as? String ?? info?["CFBundleName"] as? String ?? "Moony"
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

  // MARK: - Status bar

  /// The style for the stored theme: dark icons over the light theme, light icons over the dark
  /// one, the system's choice for anything else (nothing stored yet).
  private func statusBarStyle() -> UIStatusBarStyle {
    switch statusBarTheme {
    case "light":
      return .darkContent
    case "dark":
      return .lightContent
    default:
      return .default
    }
  }

  /// Main thread only. Makes the view controller report the stored theme's style, then asks UIKit
  /// to read it again.
  private func applyStatusBarStyle() {
    guard let controller = manager.viewController else {
      Logger.debug("status bar style kept: no view controller yet", category: logCategory)
      return
    }
    installStatusBarStyleHook(on: controller)
    controller.setNeedsStatusBarAppearanceUpdate()
  }

  /// The system samples the page under the status bar and, on iOS 26, keeps the style it sampled
  /// before a theme switch repainted (light icons over a light page until the next scroll); before
  /// iOS 26 it does not sample at all. An explicit `preferredStatusBarStyle` settles both. Tauri's
  /// view controller (tao's `TaoUIViewController`) does not override it, so the getter is
  /// replaced once on that class, with a block that reads `statusBarTheme`. Never on
  /// `UIViewController` itself: that would restyle every view controller in the process, the
  /// system's own included.
  private func installStatusBarStyleHook(on controller: UIViewController) {
    let controllerClass: AnyClass = type(of: controller)
    guard controllerClass != UIViewController.self else {
      Logger.error(
        "status bar style not set: the view controller is a plain UIViewController",
        category: logCategory)
      return
    }
    let identifier = ObjectIdentifier(controllerClass)
    guard statusBarHookClass != identifier else { return }

    let selector = #selector(getter: UIViewController.preferredStatusBarStyle)
    guard let inherited = class_getInstanceMethod(UIViewController.self, selector),
      let encoding = method_getTypeEncoding(inherited)
    else {
      Logger.error(
        "status bar style not set: preferredStatusBarStyle not found", category: logCategory)
      return
    }
    // The block is `-preferredStatusBarStyle` (an NSInteger enum) with `self` as its argument.
    let reader: @convention(block) (UIViewController) -> Int = { [weak self] _ in
      (self?.statusBarStyle() ?? .default).rawValue
    }
    class_replaceMethod(
      controllerClass, selector, imp_implementationWithBlock(reader), encoding)
    statusBarHookClass = identifier
    Logger.debug(
      "status bar style hooked on \(NSStringFromClass(controllerClass))", category: logCategory)
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
      // The decoding error can quote the argument values (a key or a reason): log its type only.
      // `Swift.type(of:)`: the `type` parameter shadows the global function here.
      reject(invoke, .failed, "Invalid arguments: \(String(describing: Swift.type(of: error)))")
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
