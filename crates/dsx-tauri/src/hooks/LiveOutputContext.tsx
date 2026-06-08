import { createContext, type Accessor } from 'solid-js'

interface LiveOutputCtx {
  liveOutputs: Accessor<Record<string, string>>
  notifyResize: () => void
}

export const LiveOutputContext = createContext<LiveOutputCtx>()
