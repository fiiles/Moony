/** A first-run step's title and lead; the wizard focuses its `h1` when it mounts. */
export function StepHead({ title, lead }: { title: string; lead: string }) {
  return (
    <>
      <h1
        tabIndex={-1}
        className="m-0 text-[26px] font-650 tracking-[-0.05em] text-ink outline-none"
      >
        {title}
      </h1>
      <p className="mb-6 mt-2 text-body leading-[1.5] text-ink-3">{lead}</p>
    </>
  );
}
