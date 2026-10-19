import { describe, expect, it } from 'vitest';
import { createReceivedQueue, createSerialDrain } from './received-queue';

function setup() {
  const delivered: string[] = [];
  const queue = createReceivedQueue((path) => delivered.push(path));
  return { queue, delivered };
}

describe('createReceivedQueue', () => {
  it('delivers_each_path_once_when_drain_and_event_overlap', () => {
    const { queue, delivered } = setup();
    // The startup drain and a "received-files" event hand over the same paths, in either order
    queue.offer(['/inbox/a.xml', '/inbox/b.xml']);
    queue.offer(['/inbox/b.xml', '/inbox/c.xml']);
    queue.offer(['/inbox/a.xml']);
    queue.sheetClosed();
    queue.sheetClosed();
    queue.sheetClosed();
    queue.sheetClosed();
    expect(delivered).toEqual(['/inbox/a.xml', '/inbox/b.xml', '/inbox/c.xml']);
  });

  it('delivers_a_path_at_once_when_nothing_is_open', () => {
    const { queue, delivered } = setup();
    queue.offer(['/inbox/a.xml']);
    expect(delivered).toEqual(['/inbox/a.xml']);
  });

  it('waits_for_the_sheet_before_the_next_path', () => {
    const { queue, delivered } = setup();
    queue.offer(['/inbox/a.xml', '/inbox/b.xml']);
    // The first opened the sheet; the second waits for it to close, however much is offered
    expect(delivered).toEqual(['/inbox/a.xml']);
    queue.offer(['/inbox/c.xml']);
    expect(delivered).toEqual(['/inbox/a.xml']);
    queue.sheetClosed();
    expect(delivered).toEqual(['/inbox/a.xml', '/inbox/b.xml']);
    queue.sheetClosed();
    expect(delivered).toEqual(['/inbox/a.xml', '/inbox/b.xml', '/inbox/c.xml']);
  });

  it('a_path_offered_again_later_is_not_redelivered', () => {
    const { queue, delivered } = setup();
    queue.offer(['/inbox/a.xml']);
    // A late event or a second drain hands over the file whose sheet is still open
    queue.offer(['/inbox/a.xml']);
    queue.offer(['/inbox/a.xml']);
    expect(delivered).toEqual(['/inbox/a.xml']);
    queue.sheetClosed();
    expect(delivered).toEqual(['/inbox/a.xml']);
  });

  it('a_file_shared_again_under_the_same_name_opens_again_once_its_sheet_closed', () => {
    const { queue, delivered } = setup();
    queue.offer(['/inbox/Transactions.csv']);
    queue.sheetClosed();
    // The copy was deleted with the sheet; the next share of the bank's export lands on the same path
    queue.offer(['/inbox/Transactions.csv']);
    expect(delivered).toEqual(['/inbox/Transactions.csv', '/inbox/Transactions.csv']);
  });

  it('holds_a_path_while_a_sheet_the_person_opened_is_open', () => {
    const { queue, delivered } = setup();
    // Importing by hand, or the app locked: nothing of ours is open, but the sheet is taken
    queue.sheetOpened();
    queue.offer(['/inbox/a.xml']);
    expect(delivered).toEqual([]);
    queue.sheetClosed();
    expect(delivered).toEqual(['/inbox/a.xml']);
  });

  it('a_sheet_that_closes_with_nothing_waiting_delivers_nothing', () => {
    const { queue, delivered } = setup();
    queue.sheetClosed();
    queue.sheetClosed();
    expect(delivered).toEqual([]);
    queue.offer([]);
    expect(delivered).toEqual([]);
  });
});

describe('createSerialDrain', () => {
  /** A run the test finishes by hand. */
  function setupDrain() {
    const finishers: Array<() => void> = [];
    let started = 0;
    let running = 0;
    let overlapped = false;
    const drain = createSerialDrain(
      () =>
        new Promise<void>((resolve) => {
          started++;
          running++;
          if (running > 1) overlapped = true;
          finishers.push(() => {
            running--;
            resolve();
          });
        })
    );
    const finish = async () => {
      finishers.shift()?.();
      // Let the loop see the run end and start the next
      await Promise.resolve();
      await Promise.resolve();
    };
    return { drain, finish, started: () => started, overlapped: () => overlapped };
  }

  it('runs_one_drain_at_a_time_and_one_more_for_any_number_of_requests_meanwhile', async () => {
    const { drain, finish, started, overlapped } = setupDrain();
    drain.request();
    expect(started()).toBe(1);
    // An event and another event while the first take is still running
    drain.request();
    drain.request();
    expect(started()).toBe(1);
    await finish();
    expect(started()).toBe(2);
    await finish();
    expect(started()).toBe(2);
    expect(overlapped()).toBe(false);
  });

  it('runs_again_for_a_request_after_the_drain_went_idle', async () => {
    const { drain, finish, started } = setupDrain();
    drain.request();
    await finish();
    expect(started()).toBe(1);
    drain.request();
    expect(started()).toBe(2);
    await finish();
  });

  it('keeps_going_after_a_failed_run', async () => {
    let runs = 0;
    const drain = createSerialDrain(async () => {
      runs++;
      if (runs === 1) throw new Error('the command failed');
    });
    drain.request();
    await Promise.resolve();
    await Promise.resolve();
    drain.request();
    await Promise.resolve();
    expect(runs).toBe(2);
  });
});
