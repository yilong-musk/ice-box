// SPDX-License-Identifier: GPL-3.0-or-later

import * as React from "react"

/** Windows narrower than this keep the phone tab bar. Wider phone-shell windows use the sidebar. */
const MOBILE_BREAKPOINT = 768

function viewportIsNarrow(breakpoint: number) {
  return window.innerWidth < breakpoint
}

/** True below `breakpoint` CSS px (768 by default: the phone/tablet split). */
export function useIsMobile(breakpoint = MOBILE_BREAKPOINT) {
  const [isMobile, setIsMobile] = React.useState(() => viewportIsNarrow(breakpoint))

  React.useEffect(() => {
    const mql = window.matchMedia(`(max-width: ${breakpoint - 1}px)`)
    const onChange = () => {
      setIsMobile(viewportIsNarrow(breakpoint))
    }
    mql.addEventListener("change", onChange)
    setIsMobile(viewportIsNarrow(breakpoint))
    return () => mql.removeEventListener("change", onChange)
  }, [breakpoint])

  return isMobile
}
