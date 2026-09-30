// SPDX-License-Identifier: GPL-3.0-or-later

import * as React from "react"

/** Windows narrower than this keep the phone tab bar. Wider phone-shell windows use the sidebar. */
const MOBILE_BREAKPOINT = 768

function viewportIsNarrow() {
  return window.innerWidth < MOBILE_BREAKPOINT
}

export function useIsMobile() {
  const [isMobile, setIsMobile] = React.useState(viewportIsNarrow)

  React.useEffect(() => {
    const mql = window.matchMedia(`(max-width: ${MOBILE_BREAKPOINT - 1}px)`)
    const onChange = () => {
      setIsMobile(viewportIsNarrow())
    }
    mql.addEventListener("change", onChange)
    setIsMobile(viewportIsNarrow())
    return () => mql.removeEventListener("change", onChange)
  }, [])

  return isMobile
}
