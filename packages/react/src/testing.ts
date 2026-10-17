import type { ReactNode } from "react"
import type { EventPayload } from "@gpuix/native"
import {
  TestRenderer,
  hasNativeTestRenderer,
  type TestWindowOptions,
} from "@gpuix/native/testing"
import { createRoot, flushSync, type Root } from "./reconciler/reconciler.js"
import { handleGpuixEvent } from "./reconciler/event-registry.js"

export * from "@gpuix/native/testing"

export interface TestRoot {
  root: Root
  renderer: TestRenderer
  render: (node: ReactNode) => void
  unmount: () => void
}

export function createTestRoot(options: TestWindowOptions = {}): TestRoot {
  let renderer!: TestRenderer
  renderer = new TestRenderer({
    ...options,
    dispatchEvent: (event: EventPayload) => {
      flushSync(() => handleGpuixEvent(event, renderer))
    },
  })
  const root = createRoot(renderer, {
    onKeyDown: options.onKeyDown,
    onEvent: options.onEvent,
    onKeyUp: options.onKeyUp,
    onSelectionChange: options.onSelectionChange,
    tabNavigation: options.tabNavigation,
    keyboardFocusDim: options.keyboardFocusDim,
  })
  const render = (node: ReactNode): void => {
    flushSync(() => root.render(node))
    renderer.flush()
  }
  let mounted = true
  return {
    root, renderer, render,
    unmount: () => {
      if (!mounted) return
      mounted = false
      try {
        root.unmount()
      } finally {
        renderer.dispose()
      }
    },
  }
}

export { TestRenderer, hasNativeTestRenderer }
