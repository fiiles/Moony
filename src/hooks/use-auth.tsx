/**
 * Auth Hook - Tauri Version
 * Supports 2-phase setup, unlock, lock, and recovery flows using Tauri invoke
 *
 * 2-Phase Flows:
 * - Setup: prepareSetup() → show recovery key → confirmSetup() → account created
 * - Recovery: prepareRecover() → show new recovery key → confirmRecover() → password changed
 */
import { createContext, ReactNode, useContext, useState, useEffect, useRef } from 'react';
import { invalidateAfterPriceRefresh } from '@/lib/price-refresh';
import { useMutation, useQuery } from '@tanstack/react-query';
import { authApi, biometricApi, priceApi, categorizationApi, demoApi } from '../lib/tauri-api';
import { queryClient } from '../lib/queryClient';
import { toast } from 'sonner';
import { useTranslation } from 'react-i18next';
import { translateApiError } from '@/lib/translate-api-error';
import { isAuthApiError } from '@/lib/api-error';
import { isMobileShell } from '@/lib/platform';
import type { WizardEntry } from '@/utils/demo-profile';
import type { DemoStatus, UserProfile } from '@shared/schema';
import type { MutateOptions, UseMutationResult } from '@tanstack/react-query';

type AppStatus = 'needs_setup' | 'locked' | 'unlocked';

/**
 * The demo profile (ADR 0037) is a desktop feature: the phone keeps today's first run and never
 * asks for the demo's state.
 */
export const DEMO_AVAILABLE = !isMobileShell(import.meta.env);

/** What the choice screen needs to know about a stored demo while no profile is open. */
export interface StoredDemo {
  exists: boolean;
  /** Unix seconds; null when unknown. */
  lastOpenedAt: number | null;
}

/** Where leaving the demo goes: the choice screen, or the wizard's first step ("Založit vlastní profil"). */
export type DemoExit = 'choice' | 'ownProfile';

// Pending setup data (stored between prepare and confirm phases)
interface PendingSetup {
  name: string;
  surname: string;
  email: string;
  password: string;
  language?: string;
  currency?: string;
}

// Pending recovery data (stored between prepare and confirm phases)
interface PendingRecover {
  oldRecoveryKey: string;
  newPassword: string;
  newRecoveryKey: string;
}

type AuthContextType = {
  user: UserProfile | null;
  appStatus: AppStatus;
  isLoading: boolean;
  error: Error | null;
  setupMutation: UseMutationResult<
    PendingSetup & { recoveryKey: string },
    Error,
    {
      name: string;
      surname: string;
      email?: string;
      password: string;
      language?: string;
      currency?: string;
    }
  >;
  /**
   * Takes no variables, so the password never sits in the mutation's state (`variables` stays
   * undefined); read its status, `submittedAt` and `isSuccess`, and submit through `unlockWithPassword`.
   */
  unlockMutation: UseMutationResult<UserProfile, Error, void>;
  /** Starts `unlockMutation` with this password; the password is held in a ref only until the unlock settles. */
  unlockWithPassword: (
    data: { password: string },
    options?: MutateOptions<UserProfile, Error, void>
  ) => void;
  /**
   * Phones and the desktop (macOS, Windows): unlock with the key stored behind biometry; the
   * variable is the prompt text.
   */
  unlockWithBiometricsMutation: UseMutationResult<UserProfile, Error, string>;
  lockMutation: UseMutationResult<void, Error, void>;
  recoverMutation: UseMutationResult<
    { recoveryKey: string; newPassword: string; newRecoveryKey: string },
    Error,
    { recoveryKey: string; newPassword: string }
  >;
  confirmSetupMutation: UseMutationResult<UserProfile, Error, void>;
  confirmRecoveryMutation: UseMutationResult<UserProfile, Error, void>;
  recoveryKey: string | null;
  clearRecoveryKey: () => void;
  pendingSetup: PendingSetup | null;
  pendingRecover: PendingRecover | null;
  /** The open profile is the demo (Alex Meyer's synthetic data), not a real profile. */
  isDemo: boolean;
  /** The stored demo while no real profile exists; null on the phone or with a real profile. */
  storedDemo: StoredDemo | null;
  /** How the first-run wizard was entered (desktop); null while it is not shown. */
  wizardEntry: WizardEntry | null;
  enterWizard: (entry: WizardEntry) => void;
  leaveWizard: () => void;
  /** Opens (or first generates) the demo in this language and enters the app with no password. */
  openDemoMutation: UseMutationResult<UserProfile, Error, string>;
  /** Ends the demo session (the demo stays stored), then the choice screen or the wizard. */
  closeDemoMutation: UseMutationResult<DemoStatus | null, Error, DemoExit>;
  /** Throws the open demo away and generates it again in the profile's language. */
  resetDemoMutation: UseMutationResult<UserProfile, Error, void>;
  /** Deletes the stored demo ("Začít znovu s původními daty"). */
  discardDemoMutation: UseMutationResult<void, Error, void>;
};

const AuthContext = createContext<AuthContextType | null>(null);

/** localStorage keys the lock screen reads while the database is still encrypted. */
export const GREETING_NAME_KEY = 'moony-greeting-name';
export const LOCKED_AT_KEY = 'moony-locked-at';

/** Only the first name, for "Vítejte zpět, Filipe"; nothing else leaves the encrypted database. */
// eslint-disable-next-line react-refresh/only-export-components -- shared with the first run's declined path
export function rememberGreetingName(name: string | null | undefined) {
  try {
    if (name) localStorage.setItem(GREETING_NAME_KEY, name);
  } catch {
    /* private mode */
  }
}

/**
 * After a join the profile holds the synced name: read it for the lock screen's greeting. A profile
 * that cannot be read now is skipped silently; nothing waits on this.
 */
// eslint-disable-next-line react-refresh/only-export-components -- shared with both first runs
export async function rememberGreetingNameFromProfile() {
  try {
    rememberGreetingName((await authApi.getProfile())?.name);
  } catch {
    /* the next password unlock stores it */
  }
}

/** The demo's state, or null when it cannot be read (the first run then works without it). */
async function readDemoStatus(): Promise<DemoStatus | null> {
  try {
    return await demoApi.status();
  } catch (error) {
    console.error('Failed to read the demo profile status:', error);
    return null;
  }
}

function storedDemoFrom(status: DemoStatus | null): StoredDemo {
  return status
    ? { exists: status.exists, lastOpenedAt: status.lastOpenedAt }
    : { exists: false, lastOpenedAt: null };
}

function rememberLockedAt() {
  try {
    localStorage.setItem(LOCKED_AT_KEY, String(Date.now()));
  } catch {
    /* private mode */
  }
}

export function AuthProvider({ children }: { children: ReactNode }) {
  const { t } = useTranslation('auth');
  const { t: tc } = useTranslation('common');
  const [recoveryKey, setRecoveryKey] = useState<string | null>(null);
  const [pendingSetup, setPendingSetup] = useState<PendingSetup | null>(null);
  const [pendingRecover, setPendingRecover] = useState<PendingRecover | null>(null);
  const [appStatus, setAppStatus] = useState<AppStatus>('locked');
  const [isCheckingStatus, setIsCheckingStatus] = useState(true);
  const [error, _setError] = useState<Error | null>(null);
  const [isDemo, setIsDemo] = useState(false);
  const [storedDemo, setStoredDemo] = useState<StoredDemo | null>(null);
  const [wizardEntry, setWizardEntry] = useState<WizardEntry | null>(null);

  // Fetch user profile automatically when unlocked
  const { data: user = null, isLoading: isProfileLoading } = useQuery({
    queryKey: ['user-profile'],
    queryFn: () => authApi.getProfile(),
    enabled: appStatus === 'unlocked',
    staleTime: Infinity, // Keep data fresh unless invalidated
  });

  const isLoading = isCheckingStatus || (appStatus === 'unlocked' && isProfileLoading && !user);

  // Check initial app status
  useEffect(() => {
    const checkStatus = async () => {
      try {
        setIsCheckingStatus(true);
        const hasSetup = await authApi.checkSetup();

        if (!hasSetup) {
          // No real profile: the demo may be open (a webview reload while in it) or stored.
          const demo = DEMO_AVAILABLE ? await readDemoStatus() : null;
          if (demo?.active) {
            setIsDemo(true);
            setStoredDemo(storedDemoFrom(demo));
            setAppStatus('unlocked');
          } else {
            if (demo) setStoredDemo(storedDemoFrom(demo));
            setAppStatus('needs_setup');
          }
        } else {
          // Check if authenticated
          const isAuth = await authApi.isAuthenticated();
          if (isAuth) {
            setAppStatus('unlocked');
          } else {
            setAppStatus('locked');
          }
        }
      } catch (err) {
        console.error('Failed to check app status:', err);
        setAppStatus('locked');
      } finally {
        setIsCheckingStatus(false);
      }
    };

    checkStatus();
  }, []);

  // Phase 1: Prepare setup - generates keys, shows recovery key, but doesn't create account
  const setupMutation = useMutation({
    mutationFn: async (data: {
      name: string;
      surname: string;
      email?: string;
      password: string;
      language?: string;
      currency?: string;
    }) => {
      // The backend keeps the generated keys; only the recovery key comes back, to show the user
      const { recoveryKey } = await authApi.prepareSetup();
      return {
        ...data,
        email: data.email || '',
        recoveryKey,
      };
    },
    onSuccess: (result) => {
      // Store pending setup data
      setPendingSetup({
        name: result.name,
        surname: result.surname,
        email: result.email,
        password: result.password,
        language: result.language,
        currency: result.currency,
      });
      // Show recovery key modal
      setRecoveryKey(result.recoveryKey);
      toast(t('toast.almostDone'), { description: t('toast.saveRecoveryKey') });
    },
    onError: (error: Error) => {
      toast.error(t('toast.setupFailed'), { description: translateApiError(error, tc) });
    },
  });

  // Phase 2: Confirm setup - actually creates the account
  const confirmSetupMutation = useMutation({
    mutationFn: async () => {
      if (!pendingSetup) {
        throw new Error('No pending setup to confirm');
      }
      // Now actually create the account
      const profile = await authApi.confirmSetup(pendingSetup);
      return profile;
    },
    onSuccess: (profile) => {
      queryClient.setQueryData(['user-profile'], profile);
      setAppStatus('unlocked');
      // The first lock after setup greets by name too, not only locks after an unlock.
      rememberGreetingName(profile.name);
      setPendingSetup(null);
      // The backend deleted a stored demo now that a real profile exists (ADR 0037).
      setStoredDemo(null);
      // Load learned payees from database
      categorizationApi.loadFromDb().catch(console.error);
      // Load user's own IBANs for internal transfer detection
      categorizationApi.loadOwnIbans().catch(console.error);
      // Load custom categorization rules from database
      categorizationApi.loadCustomRulesFromDb().catch(console.error);
      // Load locale rule packs into the engine (default rules layer)
      categorizationApi.loadRulePacksConfig().catch(console.error);
      // Recovery key will be cleared by auth-page when user dismisses modal
      toast(t('toast.setupComplete'), { description: t('toast.setupCompleteDesc') });
    },
    onError: (error: Error) => {
      toast.error(t('toast.setupFailed'), { description: translateApiError(error, tc) });
    },
  });

  // After an unlock (password or biometrics): the profile, then prices and the categorization
  // engine in the background
  const finishUnlock = (profile: UserProfile) => {
    queryClient.setQueryData(['user-profile'], profile);
    setAppStatus('unlocked');
    rememberGreetingName(profile.name);
    refreshAfterOpen();
  };

  // The background work after a profile opens (an unlock, the demo): prices, dividends and the
  // categorization engine
  const refreshAfterOpen = () => {
    // Refresh all prices in background after unlock
    // Stock prices and dividends (Yahoo Finance)
    priceApi
      .refreshStockPrices()
      .then(() => invalidateAfterPriceRefresh(queryClient))
      .catch(console.error);
    priceApi
      .refreshDividends()
      .then(() => {
        queryClient.invalidateQueries({ queryKey: ['investments'] });
        queryClient.invalidateQueries({ queryKey: ['dividend-summary'] });
        queryClient.invalidateQueries({ queryKey: ['stock-dividends'] });
      })
      .catch(console.error);
    // Crypto prices (Yahoo Finance)
    priceApi
      .refreshCryptoPrices()
      .then(() => invalidateAfterPriceRefresh(queryClient))
      .catch(console.error);

    // Load learned payees from database for categorization
    categorizationApi.loadFromDb().catch(console.error);
    // Load user's own IBANs for internal transfer detection
    categorizationApi.loadOwnIbans().catch(console.error);
    // Load custom categorization rules from database
    categorizationApi.loadCustomRulesFromDb().catch(console.error);
    // Load locale rule packs into the engine (default rules layer)
    categorizationApi.loadRulePacksConfig().catch(console.error);
  };

  // Credential failures (auth.wrongPassword, auth.keyFilesCorrupted, auth.biometric*, ...) are
  // shown inline by the lock screen; only other failures toast.
  const toastUnlockFailure = (error: Error) => {
    if (isAuthApiError(error)) return;
    toast.error(t('toast.unlockFailed'), { description: translateApiError(error, tc) });
  };

  // Unlock mutation (login with password). It takes no variables: TanStack Query keeps a
  // mutation's variables in its state (and the mutation cache) after it settles, so the password
  // travels in a ref that `mutationFn` empties in `finally`.
  const unlockPasswordRef = useRef<string | null>(null);
  const unlockMutation = useMutation<UserProfile, Error, void>({
    mutationFn: async () => {
      const password = unlockPasswordRef.current;
      unlockPasswordRef.current = null;
      if (password === null) throw new Error('No password to unlock with');
      return authApi.unlock(password);
    },
    onSuccess: finishUnlock,
    onError: toastUnlockFailure,
  });
  const unlockWithPassword = (
    data: { password: string },
    options?: MutateOptions<UserProfile, Error, void>
  ) => {
    unlockPasswordRef.current = data.password;
    unlockMutation.mutate(undefined, options);
  };

  // Phones and the desktop: unlock with the master key stored behind Face ID / Touch ID /
  // Windows Hello (it never reaches the webview); the same follow-ups as a password unlock
  const unlockWithBiometricsMutation = useMutation({
    mutationFn: (reason: string) => biometricApi.unlock(reason),
    onSuccess: finishUnlock,
    onError: toastUnlockFailure,
  });

  // Lock mutation (logout)
  const lockMutation = useMutation({
    mutationFn: async () => {
      await authApi.logout();
    },
    onSuccess: () => {
      queryClient.clear();
      // A lock that slips through in the demo ends the demo in Rust: back to the choice screen.
      if (isDemo) {
        leaveDemoState();
        return;
      }
      setAppStatus('locked');
      rememberLockedAt();
    },
    onError: (error: Error) => {
      toast.error(t('toast.lockFailed'), { description: translateApiError(error, tc) });
    },
  });

  // Phase 1: Prepare recovery - verifies old recovery key, generates new one
  const recoverMutation = useMutation({
    mutationFn: async (data: { recoveryKey: string; newPassword: string }) => {
      // Call prepare_recover to verify and get new recovery key
      const result = await authApi.prepareRecover(data);
      return { ...data, newRecoveryKey: result.recoveryKey };
    },
    onSuccess: (result) => {
      // Store pending recovery data
      setPendingRecover({
        oldRecoveryKey: result.recoveryKey,
        newPassword: result.newPassword,
        newRecoveryKey: result.newRecoveryKey,
      });
      // Show new recovery key modal
      setRecoveryKey(result.newRecoveryKey);
      toast(t('toast.recoveryKeyVerified'), { description: t('toast.saveNewRecoveryKey') });
    },
    onError: (error: Error) => {
      // A wrong recovery key is shown inline under the key field by the auth page.
      if (isAuthApiError(error)) return;
      toast.error(t('toast.recoveryFailed'), { description: translateApiError(error, tc) });
    },
  });

  // Phase 2: Confirm recovery - actually changes password and recovery key
  const confirmRecoveryMutation = useMutation({
    mutationFn: async () => {
      if (!pendingRecover) {
        throw new Error('No pending recovery to confirm');
      }
      // Now actually change the password
      const profile = await authApi.confirmRecover(pendingRecover);
      return profile;
    },
    onSuccess: (profile) => {
      queryClient.setQueryData(['user-profile'], profile);
      setAppStatus('unlocked');
      rememberGreetingName(profile.name);
      setPendingRecover(null);
      // Load learned payees from database
      categorizationApi.loadFromDb().catch(console.error);
      // Load user's own IBANs for internal transfer detection
      categorizationApi.loadOwnIbans().catch(console.error);
      // Load custom categorization rules from database
      categorizationApi.loadCustomRulesFromDb().catch(console.error);
      // Load locale rule packs into the engine (default rules layer)
      categorizationApi.loadRulePacksConfig().catch(console.error);
      // Recovery key will be cleared by auth-page
      toast(t('toast.recoverySuccess'), { description: t('toast.newRecoveryKey') });
    },
    onError: (error: Error) => {
      toast.error(t('toast.recoveryFailed'), { description: translateApiError(error, tc) });
    },
  });

  // ---------------- Demo profile (ADR 0037) ----------------

  // The demo is no longer the open profile (closed, a failed reset, a lock): the choice screen
  // reads the stored demo afresh.
  const leaveDemoState = () => {
    setIsDemo(false);
    setAppStatus('needs_setup');
    void readDemoStatus().then((status) => setStoredDemo(storedDemoFrom(status)));
  };

  const openDemoMutation = useMutation<UserProfile, Error, string>({
    mutationFn: (language) => demoApi.open(language),
    onSuccess: (profile) => {
      setWizardEntry(null);
      setIsDemo(true);
      setStoredDemo({ exists: true, lastOpenedAt: Math.floor(Date.now() / 1000) });
      queryClient.setQueryData(['user-profile'], profile);
      setAppStatus('unlocked');
      refreshAfterOpen();
    },
    onError: (error) => {
      toast.error(t('demo.toast.openFailed'), { description: translateApiError(error, tc) });
    },
  });

  const closeDemoMutation = useMutation<DemoStatus | null, Error, DemoExit>({
    mutationFn: async () => {
      await demoApi.close();
      return readDemoStatus();
    },
    onSuccess: (status, exit) => {
      // The whole dataset goes, as on a lock
      queryClient.clear();
      setIsDemo(false);
      setStoredDemo(status ? storedDemoFrom(status) : { exists: true, lastOpenedAt: null });
      setWizardEntry(exit === 'ownProfile' ? { from: 'demo', path: 'new' } : null);
      setAppStatus('needs_setup');
    },
    onError: (error) => {
      toast.error(t('demo.toast.closeFailed'), { description: translateApiError(error, tc) });
    },
  });

  const resetDemoMutation = useMutation<UserProfile, Error, void>({
    mutationFn: () => demoApi.reset(),
    onSuccess: (profile) => {
      setStoredDemo({ exists: true, lastOpenedAt: Math.floor(Date.now() / 1000) });
      queryClient.setQueryData(['user-profile'], profile);
      // A new dataset under the same open profile: every other query starts over (active ones
      // refetch). The profile stays, so the shell does not fall back to the auth page meanwhile.
      void queryClient.resetQueries({
        predicate: (query) => query.queryKey[0] !== 'user-profile',
      });
      refreshAfterOpen();
    },
    onError: (error) => {
      toast.error(t('demo.toast.resetFailed'), { description: translateApiError(error, tc) });
      // A failed regeneration leaves the demo closed in Rust
      queryClient.clear();
      leaveDemoState();
    },
  });

  const discardDemoMutation = useMutation<void, Error, void>({
    mutationFn: () => demoApi.discard(),
    onSuccess: () => {
      setStoredDemo({ exists: false, lastOpenedAt: null });
      // Discarding closes an open demo first
      if (isDemo) {
        queryClient.clear();
        setIsDemo(false);
        setAppStatus('needs_setup');
      }
    },
    onError: (error) => {
      toast.error(t('demo.toast.discardFailed'), { description: translateApiError(error, tc) });
    },
  });

  const enterWizard = (entry: WizardEntry) => setWizardEntry(entry);
  const leaveWizard = () => setWizardEntry(null);

  const clearRecoveryKey = () => {
    setRecoveryKey(null);
    // If user cancels during setup before confirming, clear pending setup
    if (pendingSetup && appStatus === 'needs_setup') {
      setPendingSetup(null);
    }
    // If user cancels during recovery before confirming, clear pending recover
    if (pendingRecover && appStatus === 'locked') {
      setPendingRecover(null);
    }
  };

  return (
    <AuthContext.Provider
      value={{
        user,
        appStatus,
        isLoading,
        error,
        setupMutation,
        unlockMutation,
        unlockWithPassword,
        unlockWithBiometricsMutation,
        lockMutation,
        recoverMutation,
        confirmSetupMutation,
        confirmRecoveryMutation,
        recoveryKey,
        clearRecoveryKey,
        pendingSetup,
        pendingRecover,
        isDemo,
        storedDemo,
        wizardEntry,
        enterWizard,
        leaveWizard,
        openDemoMutation,
        closeDemoMutation,
        resetDemoMutation,
        discardDemoMutation,
      }}
    >
      {children}
    </AuthContext.Provider>
  );
}

// eslint-disable-next-line react-refresh/only-export-components -- context hook lives with its provider
export function useAuth() {
  const context = useContext(AuthContext);
  if (!context) {
    throw new Error('useAuth must be used within an AuthProvider');
  }
  return context;
}
