/**
 * Auth Hook - Tauri Version
 * Supports 2-phase setup, unlock, lock, and recovery flows using Tauri invoke
 *
 * 2-Phase Flows:
 * - Setup: prepareSetup() → show recovery key → confirmSetup() → account created
 * - Recovery: prepareRecover() → show new recovery key → confirmRecover() → password changed
 */
import { createContext, ReactNode, useContext, useState, useEffect } from 'react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { authApi, priceApi, categorizationApi } from '../lib/tauri-api';
import { queryClient } from '../lib/queryClient';
import { toast } from 'sonner';
import { useTranslation } from 'react-i18next';
import { translateApiError } from '@/lib/translate-api-error';
import { isAuthApiError } from '@/lib/api-error';
import type { UserProfile } from '@shared/schema';
import type { UseMutationResult } from '@tanstack/react-query';

type AppStatus = 'needs_setup' | 'locked' | 'unlocked';

// Pending setup data (stored between prepare and confirm phases)
interface PendingSetup {
  name: string;
  surname: string;
  email: string;
  password: string;
  masterKeyHex: string;
  recoveryKey: string;
  salt: number[];
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
    PendingSetup,
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
  unlockMutation: UseMutationResult<UserProfile, Error, { password: string }>;
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
};

const AuthContext = createContext<AuthContextType | null>(null);

/** localStorage keys the lock screen reads while the database is still encrypted. */
export const GREETING_NAME_KEY = 'moony-greeting-name';
export const LOCKED_AT_KEY = 'moony-locked-at';

/** Only the first name, for "Vítejte zpět, Filipe"; nothing else leaves the encrypted database. */
function rememberGreetingName(name: string | null | undefined) {
  try {
    if (name) localStorage.setItem(GREETING_NAME_KEY, name);
  } catch {
    /* private mode */
  }
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
          setAppStatus('needs_setup');
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
      // Call prepare_setup to get keys
      const prepared = await authApi.prepareSetup();
      return {
        ...data,
        email: data.email || '',
        ...prepared,
      };
    },
    onSuccess: (result) => {
      // Store pending setup data
      setPendingSetup({
        name: result.name,
        surname: result.surname,
        email: result.email,
        password: result.password,
        masterKeyHex: result.masterKeyHex,
        recoveryKey: result.recoveryKey,
        salt: result.salt,
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
      setPendingSetup(null);
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

  // Unlock mutation (login with password)
  const unlockMutation = useMutation({
    mutationFn: async (data: { password: string }) => {
      const profile = await authApi.unlock(data.password);
      return profile;
    },
    onSuccess: (profile) => {
      queryClient.setQueryData(['user-profile'], profile);
      setAppStatus('unlocked');
      rememberGreetingName(profile.name);

      // Refresh all prices in background after unlock
      // Stock prices and dividends (Yahoo Finance)
      priceApi
        .refreshStockPrices()
        .then(() => {
          queryClient.invalidateQueries({ queryKey: ['investments'] });
          queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
        })
        .catch(console.error);
      priceApi
        .refreshDividends()
        .then(() => {
          queryClient.invalidateQueries({ queryKey: ['investments'] });
          queryClient.invalidateQueries({ queryKey: ['dividend-summary'] });
        })
        .catch(console.error);
      // Crypto prices (CoinGecko)
      priceApi
        .refreshCryptoPrices()
        .then(() => {
          queryClient.invalidateQueries({ queryKey: ['crypto'] });
          queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
        })
        .catch(console.error);

      // Load learned payees from database for categorization
      categorizationApi.loadFromDb().catch(console.error);
      // Load user's own IBANs for internal transfer detection
      categorizationApi.loadOwnIbans().catch(console.error);
      // Load custom categorization rules from database
      categorizationApi.loadCustomRulesFromDb().catch(console.error);
      // Load locale rule packs into the engine (default rules layer)
      categorizationApi.loadRulePacksConfig().catch(console.error);
    },
    onError: (error: Error) => {
      // Credential failures (auth.wrongPassword, auth.keyFilesCorrupted, ...) are shown inline
      // under the password field by the auth page; only other failures toast.
      if (isAuthApiError(error)) return;
      toast.error(t('toast.unlockFailed'), { description: translateApiError(error, tc) });
    },
  });

  // Lock mutation (logout)
  const lockMutation = useMutation({
    mutationFn: async () => {
      await authApi.logout();
    },
    onSuccess: () => {
      queryClient.clear();
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
        lockMutation,
        recoverMutation,
        confirmSetupMutation,
        confirmRecoveryMutation,
        recoveryKey,
        clearRecoveryKey,
        pendingSetup,
        pendingRecover,
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
