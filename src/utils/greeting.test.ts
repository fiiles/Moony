import { describe, expect, it } from 'vitest';
import { greetingKey, greetingMessage } from './greeting';

describe('greetingKey', () => {
  it('splits the day at 5, 11 and 18', () => {
    expect(greetingKey(4)).toBe('evening');
    expect(greetingKey(5)).toBe('morning');
    expect(greetingKey(10)).toBe('morning');
    expect(greetingKey(11)).toBe('day');
    expect(greetingKey(17)).toBe('day');
    expect(greetingKey(18)).toBe('evening');
  });
});

describe('greetingMessage', () => {
  it('greets by name when the profile has one', () => {
    expect(greetingMessage(8, false, 'Filipe')).toEqual({
      key: 'greeting.morning',
      name: 'Filipe',
    });
    expect(greetingMessage(20, true, ' Alex ')).toEqual({ key: 'welcomeFirst', name: 'Alex' });
  });

  it('drops the name, and the comma with it, when there is none', () => {
    expect(greetingMessage(8, false, '')).toEqual({ key: 'greeting.morningNoName' });
    expect(greetingMessage(12, false, '  ')).toEqual({ key: 'greeting.dayNoName' });
    expect(greetingMessage(22, false, '')).toEqual({ key: 'greeting.eveningNoName' });
    expect(greetingMessage(8, true, '')).toEqual({ key: 'welcomeFirstNoName' });
  });
});

/** Every locale's greetings without a name, as the overview shows them. */
const dashboards = import.meta.glob<{
  welcomeFirstNoName: string;
  greeting: Record<string, string>;
}>('../i18n/locales/*/dashboard.json', { eager: true, import: 'default' });

describe('the greetings without a name', () => {
  it.each(Object.entries(dashboards))('%s names nobody and leaves no lone comma', (_, d) => {
    const texts = [
      d.welcomeFirstNoName,
      d.greeting.morningNoName,
      d.greeting.dayNoName,
      d.greeting.eveningNoName,
    ];
    for (const text of texts) {
      expect(text).toBeTruthy();
      expect(text).not.toContain('{{name}}');
      expect(text).not.toMatch(/[,，、]\s*[.。]?$/);
    }
  });
});
