export type Rng = () => number

export function createRng(seed: number): Rng {
  let state = seed >>> 0
  return () => {
    state = (state + 0x6d2b79f5) >>> 0
    let t = state
    t = Math.imul(t ^ (t >>> 15), t | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

export function shuffle<T>(items: readonly T[], rng: Rng): T[] {
  const out = [...items]
  for (let index = out.length - 1; index > 0; index--) {
    const swap = Math.floor(rng() * (index + 1))
    const current = out[index]!
    out[index] = out[swap]!
    out[swap] = current
  }
  return out
}

export function quantile(values: readonly number[], q: number): number {
  if (values.length === 0) return Number.NaN
  const sorted = [...values].sort((left, right) => left - right)
  const position = (sorted.length - 1) * q
  const lower = Math.floor(position)
  const upper = Math.ceil(position)
  const weight = position - lower
  return sorted[lower]! * (1 - weight) + sorted[upper]! * weight
}

export function median(values: readonly number[]): number {
  return quantile(values, 0.5)
}

export interface Interval {
  estimate: number
  low: number
  high: number
}

/** stat(baseline) / stat(candidate) with a bootstrap 95% CI; above 1 means candidate is faster. */
export function speedupInterval(
  candidate: readonly number[],
  baseline: readonly number[],
  rng: Rng,
  resamples = 2000,
  statistic: (values: readonly number[]) => number = median
): Interval {
  const ratios: number[] = []
  for (let round = 0; round < resamples; round++) {
    ratios.push(
      statistic(resample(baseline, rng)) / statistic(resample(candidate, rng))
    )
  }
  return {
    estimate: statistic(baseline) / statistic(candidate),
    low: quantile(ratios, 0.025),
    high: quantile(ratios, 0.975),
  }
}

function resample(values: readonly number[], rng: Rng): number[] {
  const out = new Array<number>(values.length)
  for (let index = 0; index < values.length; index++) {
    out[index] = values[Math.floor(rng() * values.length)]!
  }
  return out
}
