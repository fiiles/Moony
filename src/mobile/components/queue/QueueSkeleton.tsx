/** Three rows in outline while the first answer loads. */
export function QueueSkeleton() {
  return (
    <div aria-hidden>
      <div className="skeleton mx-0.5 mb-2.5 mt-1 h-5 w-48" />
      <div className="skeleton mx-0.5 mb-3 h-1.5 w-full" />
      <div className="mb-3.5 overflow-hidden rounded-r4 border border-line bg-paper shadow-e1">
        {[0, 1, 2].map((i) => (
          <div key={i} className="px-3.5 py-2.5">
            <div className="flex min-h-[60px] items-center gap-3">
              <div className="skeleton size-9 flex-none rounded-full" />
              <div className="min-w-0 flex-1">
                <div className="skeleton h-3.5 w-2/5" />
                <div className="skeleton mt-2 h-3 w-3/5" />
              </div>
            </div>
            <div className="skeleton h-9 w-40 rounded-full" />
          </div>
        ))}
      </div>
    </div>
  );
}
