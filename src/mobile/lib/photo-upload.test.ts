import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { RealEstatePhoto, RealEstatePhotoBatch } from '@shared/schema';
import { uploadToTodayBatch, type PhotoApi, type PhotoSource } from './photo-upload';

const DAY = 86_400;
const TODAY = Date.UTC(2026, 9, 24) / 1000;

const batch = (id: string, photoDate: number): RealEstatePhotoBatch => ({
  id,
  realEstateId: 'p',
  photoDate,
  description: null,
  photos: [],
  createdAt: photoDate,
});

const photo = (batchId: string): RealEstatePhoto => ({
  id: `photo-${batchId}`,
  batchId,
  createdAt: 0,
});

/** A picked file: its bytes are what `arrayBuffer` gives, and reading can fail. */
function file(name: string, bytes: number[] = [1, 2, 3], fails = false): PhotoSource {
  return {
    name,
    arrayBuffer: () =>
      fails
        ? Promise.reject(new Error(`cannot read ${name}`))
        : Promise.resolve(new Uint8Array(bytes).buffer),
  };
}

/** The backend's four commands as mocks; `overrides` replace what a command does. */
function fakeApi(batches: RealEstatePhotoBatch[], overrides: Partial<PhotoApi> = {}) {
  return {
    getPhotoBatches: vi.fn<PhotoApi['getPhotoBatches']>(
      overrides.getPhotoBatches ?? (async () => batches)
    ),
    createPhotoBatch: vi.fn<PhotoApi['createPhotoBatch']>(
      overrides.createPhotoBatch ?? (async (_id, data) => batch('new', data.photoDate))
    ),
    addPhotoToBatch: vi.fn<PhotoApi['addPhotoToBatch']>(
      overrides.addPhotoToBatch ?? (async (batchId) => photo(batchId))
    ),
    deletePhotoBatch: vi.fn<PhotoApi['deletePhotoBatch']>(
      overrides.deletePhotoBatch ?? (async () => undefined)
    ),
  };
}

describe('uploadToTodayBatch', () => {
  beforeEach(() => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
  });
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("puts the photos into today's batch when there is one, and makes no new batch", async () => {
    const api = fakeApi([batch('today', TODAY), batch('older', TODAY - 3 * DAY)]);
    const result = await uploadToTodayBatch(api, 'p', [file('a.jpg'), file('b.jpg')], TODAY);
    expect(api.createPhotoBatch).not.toHaveBeenCalled();
    expect(api.addPhotoToBatch.mock.calls.map(([batchId]) => batchId)).toEqual(['today', 'today']);
    expect(result).toEqual({ batchId: 'today', saved: 2, skipped: 0 });
  });

  it("makes today's batch when it is missing, dated today's UTC day, and fills it", async () => {
    const api = fakeApi([batch('older', TODAY - DAY)]);
    const result = await uploadToTodayBatch(api, 'p', [file('a.jpg')], TODAY + 10 * 3600);
    expect(api.createPhotoBatch).toHaveBeenCalledTimes(1);
    expect(api.createPhotoBatch).toHaveBeenCalledWith('p', { photoDate: TODAY });
    expect(api.addPhotoToBatch.mock.calls.map(([batchId]) => batchId)).toEqual(['new']);
    expect(result.batchId).toBe('new');
    expect(api.deletePhotoBatch).not.toHaveBeenCalled();
  });

  it('reads the batches from the backend, not from a cache that may be a moment old', async () => {
    const api = fakeApi([]);
    await uploadToTodayBatch(api, 'p', [file('a.jpg')], TODAY);
    expect(api.getPhotoBatches).toHaveBeenCalledWith('p');
  });

  it("sends each photo's bytes as a Uint8Array, one at a time and in order", async () => {
    const order: string[] = [];
    let running = 0;
    let overlapped = false;
    const api = fakeApi([batch('today', TODAY)], {
      addPhotoToBatch: vi.fn(async (batchId: string, bytes: Uint8Array) => {
        running += 1;
        overlapped ||= running > 1;
        order.push(Array.from(bytes).join(','));
        await Promise.resolve();
        running -= 1;
        return photo(batchId);
      }),
    });
    await uploadToTodayBatch(
      api,
      'p',
      [file('a.jpg', [1]), file('b.jpg', [2, 2]), file('c.jpg', [3])],
      TODAY
    );
    expect(order).toEqual(['1', '2,2', '3']);
    expect(overlapped).toBe(false);
    const sent = api.addPhotoToBatch.mock.calls[0][1];
    expect(sent).toBeInstanceOf(Uint8Array);
  });

  it('reports the photo it is on before each one: 1 of 3, 2 of 3, 3 of 3', async () => {
    const api = fakeApi([batch('today', TODAY)]);
    const progress: Array<{ current: number; total: number }> = [];
    await uploadToTodayBatch(api, 'p', [file('a.jpg'), file('b.jpg'), file('c.jpg')], TODAY, (p) =>
      progress.push(p)
    );
    expect(progress).toEqual([
      { current: 1, total: 3 },
      { current: 2, total: 3 },
      { current: 3, total: 3 },
    ]);
  });

  it('skips a photo that fails and saves the rest, counting what was left out', async () => {
    const api = fakeApi([batch('today', TODAY)], {
      addPhotoToBatch: vi.fn(async (batchId: string, bytes: Uint8Array) => {
        if (bytes[0] === 9) throw new Error('not an image');
        return photo(batchId);
      }),
    });
    const result = await uploadToTodayBatch(
      api,
      'p',
      [
        file('a.jpg', [1]),
        file('bad.jpg', [9]),
        file('unreadable.jpg', [1], true),
        file('c.jpg', [3]),
      ],
      TODAY
    );
    expect(result).toEqual({ batchId: 'today', saved: 2, skipped: 2 });
  });

  it('fails when no photo was saved, with the first error, and leaves a batch that was there alone', async () => {
    const failure = new Error('not an image');
    const api = fakeApi([batch('today', TODAY)], {
      addPhotoToBatch: vi.fn(async () => {
        throw failure;
      }),
    });
    await expect(uploadToTodayBatch(api, 'p', [file('a.jpg'), file('b.jpg')], TODAY)).rejects.toBe(
      failure
    );
    expect(api.deletePhotoBatch).not.toHaveBeenCalled();
  });

  it('deletes the batch it made when none of the photos was saved: no empty batch is left behind', async () => {
    const api = fakeApi([], {
      addPhotoToBatch: vi.fn(async () => {
        throw new Error('not an image');
      }),
    });
    await expect(uploadToTodayBatch(api, 'p', [file('a.jpg')], TODAY)).rejects.toThrow(
      'not an image'
    );
    expect(api.deletePhotoBatch).toHaveBeenCalledWith('new');
  });

  it('still reports the upload error when removing the empty batch fails too', async () => {
    const api = fakeApi([], {
      addPhotoToBatch: vi.fn(async () => {
        throw new Error('not an image');
      }),
      deletePhotoBatch: vi.fn(async () => {
        throw new Error('database locked');
      }),
    });
    await expect(uploadToTodayBatch(api, 'p', [file('a.jpg')], TODAY)).rejects.toThrow(
      'not an image'
    );
  });

  it('keeps the batch it made when at least one photo went in', async () => {
    const api = fakeApi([], {
      addPhotoToBatch: vi.fn(async (batchId: string, bytes: Uint8Array) => {
        if (bytes[0] === 9) throw new Error('not an image');
        return photo(batchId);
      }),
    });
    await uploadToTodayBatch(api, 'p', [file('bad.jpg', [9]), file('ok.jpg', [1])], TODAY);
    expect(api.deletePhotoBatch).not.toHaveBeenCalled();
  });

  it('fails without touching the backend for nothing to upload', async () => {
    const api = fakeApi([]);
    await expect(uploadToTodayBatch(api, 'p', [], TODAY)).rejects.toThrow();
    expect(api.getPhotoBatches).not.toHaveBeenCalled();
    expect(api.createPhotoBatch).not.toHaveBeenCalled();
  });
});
