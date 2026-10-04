/// <reference types="vite/client" />
import { invoke } from '@tauri-apps/api/core';
import { useEffect, useState } from 'react';
import { useLocation } from 'wouter';
import { systemApi } from '@/lib/tauri-api';

// Tauri v2 compatible Aptabase tracking - bypasses outdated @aptabase/tauri package
async function aptabaseTrack(name: string, props?: Record<string, string | number>) {
  await invoke('plugin:aptabase|track_event', { name, props });
}

const CONSENT_KEY = 'moony-analytics-consent';

let availability: boolean | null = null;

/** Whether this build can send analytics at all (compiled with a key). Cached per session. */
export async function isAnalyticsAvailable(): Promise<boolean> {
  if (availability === null) {
    try {
      availability = await systemApi.hasAnalytics();
    } catch {
      availability = false;
    }
  }
  return availability;
}

/** `null` until known, then whether the consent prompt and the switch make sense in this build. */
export function useAnalyticsAvailable(): boolean | null {
  const [value, setValue] = useState<boolean | null>(availability);
  useEffect(() => {
    let active = true;
    isAnalyticsAvailable().then((result) => {
      if (active) setValue(result);
    });
    return () => {
      active = false;
    };
  }, []);
  return value;
}

export function getConsent(): boolean | null {
  const stored = localStorage.getItem(CONSENT_KEY);
  if (stored === 'true') return true;
  if (stored === 'false') return false;
  return null;
}

export function setConsent(granted: boolean) {
  localStorage.setItem(CONSENT_KEY, String(granted));
}

export async function trackEvent(name: string, props?: Record<string, string | number | boolean>) {
  const consent = getConsent();

  if (consent === true) {
    try {
      // Convert booleans to strings for Aptabase compatibility
      const safeProps: Record<string, string | number> | undefined = props
        ? Object.entries(props).reduce(
            (acc, [key, value]) => {
              acc[key] = typeof value === 'boolean' ? String(value) : value;
              return acc;
            },
            {} as Record<string, string | number>
          )
        : undefined;

      await aptabaseTrack(name, safeProps);
    } catch (error) {
      console.warn('[APTABASE] Failed to track event:', error);
    }
  }
}

// No initialization needed for backend-based plugin
export async function initAnalytics() {
  // Placeholder if we need startup logic later
}

const CONSENT_VERSION_KEY = 'moony-analytics-consent-asked-version';
const CURRENT_ANALYTICS_VERSION = '1'; // Increment when analytics changes require new consent

export function needsConsentPrompt(): boolean {
  const askedVersion = localStorage.getItem(CONSENT_VERSION_KEY);
  return askedVersion !== CURRENT_ANALYTICS_VERSION;
}

export function markConsentAsked() {
  localStorage.setItem(CONSENT_VERSION_KEY, CURRENT_ANALYTICS_VERSION);
}

export function useScreenTracking() {
  const [location] = useLocation();

  useEffect(() => {
    const screenMap: Record<string, string> = {
      '/': 'dashboard',
      '/bank-accounts': 'bank_accounts',
      '/loans': 'loans',
      '/insurance': 'insurance',
      '/stocks': 'investments',
      '/crypto': 'crypto',
      '/bonds': 'bonds',
      '/real-estate': 'real_estate',
      '/other-assets': 'other_assets',
      '/reports/budgeting': 'budgeting',
      '/reports/projection': 'projection',
      '/reports/stocks-analysis': 'stocks_analysis',
      '/reports/cashflow': 'cashflow',
      '/calculators/annuity': 'annuity_calculator',
      '/calculators/estate': 'estate_calculator',
      '/settings': 'settings',
    };

    // Check for exact match
    let screen = screenMap[location];

    // If no exact match, try prefix matching for detail pages
    if (!screen) {
      if (location.startsWith('/bank-accounts/')) screen = 'bank_account_detail';
      else if (location.startsWith('/loans/')) screen = 'loan_detail';
      else if (location.startsWith('/insurance/')) screen = 'insurance_detail';
      else if (location.startsWith('/stocks/')) screen = 'investment_detail';
      else if (location.startsWith('/crypto/')) screen = 'crypto_detail';
      else if (location.startsWith('/real-estate/')) screen = 'real_estate_detail';
      else if (location.startsWith('/settings/')) screen = 'settings_subpage';
    }

    if (screen) {
      trackEvent('screen_view', { screen });
    }
  }, [location]);
}
