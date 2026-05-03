import { createContext, useContext } from 'react'

export interface ChainProxyContextType {
  isChainMode: boolean
  setChainMode: (isChain: boolean) => void
  chainConfigData: string | null
  setChainConfigData: (data: string | null) => void
}

export const ChainProxyContext = createContext<ChainProxyContextType | null>(
  null,
)

export const useChainProxy = () => {
  // eslint-disable-next-line @eslint-react/no-use-context
  const context = useContext(ChainProxyContext)
  if (!context) {
    throw new Error('useChainProxy must be used within a ChainProxyProvider')
  }
  return context
}
