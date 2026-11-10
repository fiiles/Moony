/**
 * Who holds back navigation now (`useHoldBack`): a count, so layers that hold it at the same time
 * (the scan overlay and the busy cover) release only their own hold. Back is held while any hold is.
 */
let holds = 0;

/** Takes a hold; the returned release counts once however often it is called. */
export function holdBack(): () => void {
  holds += 1;
  let released = false;
  return () => {
    if (released) return;
    released = true;
    holds -= 1;
  };
}

/** Whether any hold is taken: the edge swipe does not start (`useEdgeSwipeBack`). */
export function backIsHeld(): boolean {
  return holds > 0;
}
