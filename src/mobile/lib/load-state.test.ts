import { describe, expect, it } from 'vitest';
import { ApiError } from '@/lib/api-error';
import { failedLoad, loadState } from './load-state';

describe('loadState', () => {
  it('waits while the first load runs', () => {
    expect(loadState({ isLoading: true, isError: false, hasData: false })).toBe('loading');
  });

  it('shows what is there, even when a later refetch failed', () => {
    expect(loadState({ isLoading: false, isError: false, hasData: true })).toBe('ready');
    expect(loadState({ isLoading: false, isError: true, hasData: true })).toBe('ready');
  });

  it('tells a failed load from a record that is not there', () => {
    expect(loadState({ isLoading: false, isError: true, hasData: false })).toBe('error');
    expect(loadState({ isLoading: false, isError: false, hasData: false })).toBe('notFound');
  });
});

describe('failedLoad', () => {
  const apiError = (kind: 'notFound' | 'database') =>
    new ApiError({ kind, message: 'x', raw: 'x' });

  it('is a failed query without an earlier answer', () => {
    expect(failedLoad({ status: 'error', data: undefined, error: apiError('database') })).toBe(
      true
    );
    expect(failedLoad({ status: 'error', data: undefined, error: new Error('x') })).toBe(true);
  });

  it('is not a failure that still shows the earlier answer, or no failure at all', () => {
    expect(failedLoad({ status: 'error', data: [], error: apiError('database') })).toBe(false);
    expect(failedLoad({ status: 'success', data: undefined, error: null })).toBe(false);
    expect(failedLoad({ status: 'pending', data: undefined, error: null })).toBe(false);
  });

  it('takes the backend’s "not found" for a missing record, not a failed load', () => {
    expect(failedLoad({ status: 'error', data: undefined, error: apiError('notFound') })).toBe(
      false
    );
  });
});
