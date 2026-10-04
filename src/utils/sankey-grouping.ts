/** Share of a side's total below which a flow is folded into "Other". */
export const SMALL_FLOW_THRESHOLD = 0.02;
/** A node is never drawn thinner than this many pixels, so it stays visible and hoverable. */
export const MIN_NODE_HEIGHT = 3;
/**
 * Nodes thinner than this get no label. Two-line labels (name + amount) are about 28 px tall
 * and neighbouring nodes are `nodePadding` (20 px) apart, so labels of two nodes that are each
 * at least this thick cannot overlap; a thinner node is still reachable through its tooltip.
 */
export const MIN_LABEL_HEIGHT = 12;

export interface Flow {
  name: string;
  total: number;
}

export interface GroupedFlow extends Flow {
  /** Only on the merged "Other" node: the flows folded into it, largest first. */
  members?: Flow[];
}

/**
 * Folds the flows of one Sankey side that are under `threshold` of that side's total into a
 * single trailing "Other" node, so many hair-thin nodes do not pile their labels on top of
 * each other. A single small flow is left alone (wrapping it in "Other" would only
 * hide its name), as are flows exactly at the threshold. `otherName` builds the node's label
 * from the number of merged flows.
 */
export function groupSmallFlows(
  flows: Flow[],
  otherName: (count: number) => string,
  threshold: number = SMALL_FLOW_THRESHOLD
): GroupedFlow[] {
  const sideTotal = flows.reduce((sum, f) => sum + f.total, 0);
  if (!(sideTotal > 0)) return flows;

  const small = flows.filter((f) => f.total / sideTotal < threshold);
  if (small.length < 2) return flows;

  const kept = flows.filter((f) => f.total / sideTotal >= threshold);
  const members = [...small].sort((a, b) => b.total - a.total);
  return [
    ...kept,
    {
      name: otherName(small.length),
      total: small.reduce((sum, f) => sum + f.total, 0),
      members,
    },
  ];
}

/** The rectangle to draw for a node: at least `MIN_NODE_HEIGHT` tall, centred where it was. */
export function nodeBox(y: number, height: number): { y: number; height: number } {
  if (height >= MIN_NODE_HEIGHT) return { y, height };
  return { y: y - (MIN_NODE_HEIGHT - height) / 2, height: MIN_NODE_HEIGHT };
}

/** Whether a node of this height gets a label. */
export function showNodeLabel(height: number): boolean {
  return height >= MIN_LABEL_HEIGHT;
}
