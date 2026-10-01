import { mmkvBench } from '../storage'
import { throughputCases } from './cases'
import { makeDataset } from './fixtures'
import { type Harness, nextFrame, openScc, prepareStores, settleScc } from './harness'
import type { ProfileRequest } from './launch'
import { createRng } from './stats'

export async function runProfile(request: ProfileRequest): Promise<void> {
  const definition = throughputCases.find((entry) => entry.id === request.caseId)
  if (definition === undefined || definition.listeners !== undefined) {
    throw new Error(`case ${request.caseId} cannot be profiled`)
  }
  const rng = createRng(1)
  const data = makeDataset(definition.scenario, rng)
  const harness: Harness = {
    stores: { scc: openScc(true), mmkv: mmkvBench },
    profile: { name: 'profile', throughputTrials: 1, latencyTrials: 1, scale: 1 },
    rng,
  }
  await prepareStores(harness, data)
  const body = definition.body(request.lib, harness.stores, data)
  console.log(`SCC_PROFILE_START case=${definition.id} lib=${request.lib}`)
  const deadline = performance.now() + request.seconds * 1000
  let offset = 0
  while (performance.now() < deadline) {
    body(definition.chunkSize, offset)
    offset += definition.chunkSize
  }
  settleScc(harness.stores.scc)
  console.log(`SCC_PROFILE_DONE operations=${offset * definition.opsPerCall}`)
  await nextFrame()
}
