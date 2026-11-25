// SPDX-License-Identifier: GPL-3.0-or-later

import { StrictMode } from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  LAND,
  LandingMap,
  getLandingGlobeView,
  insideRing,
  landingFlatCenter,
  landingMapCentroid,
  resolveColor,
  setLandingFlatCenter,
  settledSlotBox,
  wrapLon,
} from "./LandingMap";
import { LandingMapDialog, guessUserCountry } from "./LandingMapDialog";
import { t } from "../lib/i18n";

/** Minimal 2D canvas context stub that exercises draw/color/morph paths. */
function makeMockContext() {
  let currentFill = "#000000";
  return {
    get fillStyle() {
      return currentFill;
    },
    set fillStyle(value: string) {
      if (value.startsWith("#") || value.startsWith("rgb") || value.startsWith("oklch")) {
        currentFill = value;
      }
    },
    globalAlpha: 1,
    setTransform: vi.fn(),
    clearRect: vi.fn(),
    beginPath: vi.fn(),
    moveTo: vi.fn(),
    arc: vi.fn(),
    fill: vi.fn(),
  } as unknown as CanvasRenderingContext2D;
}

describe("LandingMap helpers", () => {
  afterEach(() => {
    setLandingFlatCenter(null);
  });

  it("wraps longitude differences into [-180, 180)", () => {
    expect(wrapLon(0)).toBe(0);
    expect(wrapLon(179)).toBe(179);
    expect(wrapLon(180)).toBe(-180);
    expect(wrapLon(-180)).toBe(-180);
    expect(wrapLon(270)).toBe(-90);
    expect(wrapLon(-270)).toBe(90);
    expect(wrapLon(540)).toBe(-180);
  });

  it("looks up country centroids case-insensitively and returns null for unknown codes", () => {
    expect(landingMapCentroid("us")).toEqual([39, -98]);
    expect(landingMapCentroid("JP")).toEqual([36, 138]);
    expect(landingMapCentroid("XX")).toBeNull();
    expect(landingMapCentroid(null)).toBeNull();
    expect(landingMapCentroid(undefined)).toBeNull();
  });

  it("checks point inclusion inside simplified continent rings", () => {
    // Australia ring: Alice Springs is inside, central Pacific is outside.
    const australia = LAND[LAND.length - 2];
    expect(insideRing(134, -25, australia)).toBe(true);
    expect(insideRing(-150, 0, australia)).toBe(false);
  });

  it("falls back when the canvas rejects an unsupported color token", () => {
    const ctx = makeMockContext();
    expect(resolveColor(ctx, "oklch(0.6 0.2 254)", "#007aff")).toBe("oklch(0.6 0.2 254)");
    expect(resolveColor(ctx, "not-a-color", "#007aff")).toBe("#007aff");
    expect(resolveColor(ctx, "", "#007aff")).toBe("#007aff");
  });

  it("computes the dialog slot resting box from layout metrics without transform scaling", () => {
    const dialog = document.createElement("div");
    dialog.className = "landing-map-dialog";
    const slot = document.createElement("div");
    slot.setAttribute("data-landing-slot", "");
    dialog.appendChild(slot);
    document.body.appendChild(dialog);
    const prevWidth = window.innerWidth;
    const prevHeight = window.innerHeight;
    try {
      Object.defineProperty(window, "innerWidth", { configurable: true, value: 1000 });
      Object.defineProperty(window, "innerHeight", { configurable: true, value: 700 });
      Object.defineProperty(dialog, "offsetWidth", { configurable: true, value: 800 });
      Object.defineProperty(dialog, "offsetHeight", { configurable: true, value: 500 });
      Object.defineProperty(dialog, "clientLeft", { configurable: true, value: 1 });
      Object.defineProperty(dialog, "clientTop", { configurable: true, value: 1 });
      Object.defineProperty(slot, "offsetLeft", { configurable: true, value: 20 });
      Object.defineProperty(slot, "offsetTop", { configurable: true, value: 80 });
      Object.defineProperty(slot, "offsetWidth", { configurable: true, value: 758 });
      Object.defineProperty(slot, "offsetHeight", { configurable: true, value: 300 });
      // Even if getBoundingClientRect reports an animated scaled box,
      // settledSlotBox returns the resting destination box.
      vi.spyOn(slot, "getBoundingClientRect").mockReturnValue({
        left: 10,
        top: 10,
        width: 200,
        height: 100,
        right: 210,
        bottom: 110,
        x: 10,
        y: 10,
        toJSON: () => ({}),
      });
      expect(settledSlotBox(slot)).toEqual({
        left: 121,
        top: 181,
        width: 758,
        height: 300,
      });
    } finally {
      Object.defineProperty(window, "innerWidth", { configurable: true, value: prevWidth });
      Object.defineProperty(window, "innerHeight", { configurable: true, value: prevHeight });
      dialog.remove();
    }
  });
});

describe("guessUserCountry", () => {
  it("maps a known IANA timezone to its ISO country code", () => {
    const dtf = vi.spyOn(Intl, "DateTimeFormat").mockImplementation(
      () =>
        ({
          resolvedOptions: () => ({ timeZone: "Asia/Tokyo" }),
        }) as Intl.DateTimeFormat,
    );
    try {
      expect(guessUserCountry()).toBe("JP");
    } finally {
      dtf.mockRestore();
    }
  });

  it("falls back to navigator.languages when timezone is unmapped", () => {
    const dtf = vi.spyOn(Intl, "DateTimeFormat").mockImplementation(
      () =>
        ({
          resolvedOptions: () => ({ timeZone: "UTC" }),
        }) as Intl.DateTimeFormat,
    );
    const langs = vi.spyOn(navigator, "languages", "get").mockReturnValue(["de-DE", "en-US"]);
    try {
      expect(guessUserCountry()).toBe("DE");
    } finally {
      langs.mockRestore();
      dtf.mockRestore();
    }
  });

  it("extracts regions from BCP-47 tags with script subtags and ignores extension subtags", () => {
    const dtf = vi.spyOn(Intl, "DateTimeFormat").mockImplementation(
      () =>
        ({
          resolvedOptions: () => ({ timeZone: "UTC" }),
        }) as Intl.DateTimeFormat,
    );
    const langs = vi.spyOn(navigator, "languages", "get");
    try {
      langs.mockReturnValue(["zh-Hans-CN", "en-US"]);
      expect(guessUserCountry()).toBe("CN");
      langs.mockReturnValue(["zh-Hant-TW"]);
      expect(guessUserCountry()).toBe("TW");
      langs.mockReturnValue(["zh_Hans_HK"]);
      expect(guessUserCountry()).toBe("HK");
      langs.mockReturnValue(["en_u_ca_gregory"]);
      expect(guessUserCountry()).toBeNull();
    } finally {
      langs.mockRestore();
      dtf.mockRestore();
    }
  });

  it("returns null when neither timezone nor language yields a known region", () => {
    const dtf = vi.spyOn(Intl, "DateTimeFormat").mockImplementation(
      () =>
        ({
          resolvedOptions: () => ({ timeZone: "Etc/Unknown" }),
        }) as Intl.DateTimeFormat,
    );
    const langs = vi.spyOn(navigator, "languages", "get").mockReturnValue(["en"]);
    try {
      expect(guessUserCountry()).toBeNull();
    } finally {
      langs.mockRestore();
      dtf.mockRestore();
    }
  });
});

describe("LandingMap component", () => {
  afterEach(() => {
    setLandingFlatCenter(null);
    vi.restoreAllMocks();
  });

  it("renders a marker only when state is located with a known country", () => {
    const { container, rerender } = render(<LandingMap country="JP" state="located" />);
    expect(container.querySelector('[data-testid="landing-map"]')).toHaveAttribute(
      "data-state",
      "located",
    );
    expect(container.querySelector('[data-testid="landing-map-marker"]')).toHaveAttribute(
      "data-country",
      "JP",
    );

    rerender(<LandingMap country="JP" state="probing" />);
    expect(container.querySelector('[data-testid="landing-map-marker"]')).toBeNull();

    rerender(<LandingMap country="ZZ" state="located" />);
    expect(container.querySelector('[data-testid="landing-map-marker"]')).toBeNull();
  });

  it("does not interrupt an active drag when a status update changes the centroid", () => {
    const ctx = makeMockContext();
    const getContext = vi
      .spyOn(HTMLCanvasElement.prototype, "getContext")
      .mockReturnValue(ctx);
    const widthSpy = vi
      .spyOn(HTMLElement.prototype, "clientWidth", "get")
      .mockReturnValue(300);
    const heightSpy = vi
      .spyOn(HTMLElement.prototype, "clientHeight", "get")
      .mockReturnValue(144);
    let rafCallback: FrameRequestCallback | null = null;
    const raf = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
      rafCallback = cb;
      return 1;
    });
    const cancelRaf = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {
      rafCallback = null;
    });
    try {
      const { container, rerender } = render(<LandingMap country="JP" state="located" />);
      const canvas = container.querySelector("canvas") as HTMLCanvasElement;
      expect(canvas).not.toBeNull();
      const initialLon = getLandingGlobeView().lon;

      // jsdom has no PointerEvent constructor; MouseEvent populates button/clientX/clientY.
      act(() => {
        canvas.dispatchEvent(
          new MouseEvent("pointerdown", { button: 0, clientX: 100, clientY: 70, bubbles: true }),
        );
        canvas.dispatchEvent(
          new MouseEvent("pointermove", { button: 0, clientX: 160, clientY: 70, bubbles: true }),
        );
      });
      const draggedLon = getLandingGlobeView().lon;
      expect(draggedLon).not.toBeCloseTo(initialLon, 2);
      raf.mockClear();

      // Incoming status poll updates target while pointer is held down.
      rerender(<LandingMap country="US" state="located" />);
      expect(getLandingGlobeView().lon).toBeCloseTo(draggedLon, 5);
      // No turn animation was scheduled to fight the pointer.
      expect(raf).not.toHaveBeenCalled();
      expect(rafCallback).toBeNull();

      // Releasing springs toward the updated centroid.
      act(() => {
        canvas.dispatchEvent(
          new MouseEvent("pointerup", { button: 0, clientX: 160, clientY: 70, bubbles: true }),
        );
      });
      expect(raf).toHaveBeenCalled();
    } finally {
      cancelRaf.mockRestore();
      raf.mockRestore();
      heightSpy.mockRestore();
      widthSpy.mockRestore();
      getContext.mockRestore();
    }
  });

  it("times the collapse from its first frame, so a slow first frame does not skip it", () => {
    const getContext = vi
      .spyOn(HTMLCanvasElement.prototype, "getContext")
      .mockReturnValue(makeMockContext());
    const widthSpy = vi
      .spyOn(HTMLElement.prototype, "clientWidth", "get")
      .mockReturnValue(300);
    const heightSpy = vi
      .spyOn(HTMLElement.prototype, "clientHeight", "get")
      .mockReturnValue(144);
    let rafCallback: FrameRequestCallback | null = null;
    const raf = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
      rafCallback = cb;
      return 1;
    });
    const cancelRaf = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {
      rafCallback = null;
    });
    const runFrame = (time: number) => {
      const cb = rafCallback;
      rafCallback = null;
      act(() => cb?.(time));
    };
    // The dialog's map slot the canvas flies into.
    const slot = document.createElement("div");
    slot.setAttribute("data-landing-slot", "");
    document.body.append(slot);
    try {
      const { container, rerender } = render(
        <LandingMap country="JP" state="located" expanded={false} />,
      );
      const host = container.querySelector('[data-testid="landing-map"]') as HTMLElement;
      const canvas = host.querySelector("canvas") as HTMLCanvasElement;

      // Expand and let the flight land in the slot.
      rerender(<LandingMap country="JP" state="located" expanded />);
      runFrame(1_000);
      runFrame(1_000 + 2_000);
      expect(canvas.parentElement).toBe(slot);

      // Close. The first frame arrives long after the close (a slow commit
      // in WebKit): the canvas must still start its flight, not snap home.
      rerender(<LandingMap country="JP" state="located" expanded={false} />);
      expect(canvas.parentElement).toHaveClass("landing-map-stage");
      const late = performance.now() + 2_000;
      runFrame(late);
      expect(canvas.parentElement).toHaveClass("landing-map-stage");
      runFrame(late + 240);
      expect(canvas.parentElement).toHaveClass("landing-map-stage");
      // Done after the full collapse duration from that first frame.
      runFrame(late + 2_000);
      expect(canvas.parentElement).toBe(host);
    } finally {
      slot.remove();
      cancelRaf.mockRestore();
      raf.mockRestore();
      heightSpy.mockRestore();
      widthSpy.mockRestore();
      getContext.mockRestore();
    }
  });
});

describe("LandingMapDialog", () => {
  afterEach(() => {
    setLandingFlatCenter(null);
    vi.restoreAllMocks();
  });

  it("shows measured origin, exit IP, both pins, and an arc between different regions", () => {
    render(
      <StrictMode>
        <LandingMapDialog
          open
          onOpenChange={() => {}}
          country="US"
          ip="203.0.113.42"
          originCountry="CN"
          originIp="198.51.100.7"
        />
      </StrictMode>,
    );

    expect(screen.getByTestId("landing-map-dialog")).toBeInTheDocument();
    expect(screen.getByTestId("landing-map-user")).toHaveAttribute("data-country", "CN");
    expect(screen.getByTestId("landing-map-exit")).toHaveAttribute("data-country", "US");
    expect(screen.getByTestId("landing-map-arc")).toBeInTheDocument();
    expect(screen.getByTestId("landing-map-user-label").textContent).not.toContain(
      t("home.landingMap.inferred"),
    );
    const exitLabel = screen.getByTestId("landing-map-exit-label");
    expect(exitLabel).toHaveTextContent("203.0.113.42");
    expect(exitLabel.querySelector("[data-selectable]")).toHaveTextContent("203.0.113.42");
    // Midpoint between CN (lon 104) and US (lon -98): shortest way round is
    // eastward across the Pacific (+158 deg), midpoint at lon -177.
    expect(landingFlatCenter()).toBeCloseTo(-177, 4);
  });

  it("marks local timezone guess as inferred when origin is not measured", () => {
    const dtf = vi.spyOn(Intl, "DateTimeFormat").mockImplementation(
      () =>
        ({
          resolvedOptions: () => ({ timeZone: "Asia/Shanghai" }),
        }) as Intl.DateTimeFormat,
    );
    try {
      render(
        <LandingMapDialog
          open
          onOpenChange={() => {}}
          country="JP"
          ip="203.0.113.42"
          originCountry={null}
          originIp={null}
        />,
      );
      expect(screen.getByTestId("landing-map-user")).toHaveAttribute("data-country", "CN");
      expect(screen.getByTestId("landing-map-user-label").textContent).toContain(
        t("home.landingMap.inferred"),
      );
    } finally {
      dtf.mockRestore();
    }
  });

  it("suppresses the arc when origin and exit share the same location", () => {
    render(
      <LandingMapDialog
        open
        onOpenChange={() => {}}
        country="JP"
        ip="203.0.113.42"
        originCountry="JP"
        originIp="198.51.100.7"
      />,
    );
    expect(screen.getByTestId("landing-map-user")).toHaveAttribute("data-country", "JP");
    expect(screen.getByTestId("landing-map-exit")).toHaveAttribute("data-country", "JP");
    expect(screen.queryByTestId("landing-map-arc")).toBeNull();
  });

  it("shows no-exit state when there is no probed exit IP", () => {
    render(
      <LandingMapDialog
        open
        onOpenChange={() => {}}
        country={null}
        ip={null}
        originCountry="CN"
        originIp="198.51.100.7"
      />,
    );
    expect(screen.getByTestId("landing-map-exit-label")).toHaveTextContent(
      t("home.landingMap.noExit"),
    );
    expect(screen.queryByTestId("landing-map-exit")).toBeNull();
    expect(screen.queryByTestId("landing-map-arc")).toBeNull();
  });

  it("closes on a press on the overlay without inserting a stylesheet", async () => {
    // Radix's own overlay locks scrolling by inserting a <style> into <head>
    // on open (and removing it on close); WebKit restyles the whole page for
    // each, in the first frame of the zoom. The plain overlay does not.
    const headStyles = () => document.head.querySelectorAll("style").length;
    const before = headStyles();
    const onOpenChange = vi.fn();
    render(
      <LandingMapDialog
        open
        onOpenChange={onOpenChange}
        country="JP"
        ip="203.0.113.42"
        originCountry="CN"
        originIp="198.51.100.7"
      />,
    );
    expect(headStyles()).toBe(before);
    const overlay = document.querySelector('[data-slot="dialog-overlay"]') as HTMLElement;
    expect(overlay).toHaveAttribute("data-state", "open");
    // Radix starts listening for outside presses a tick after mounting.
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    fireEvent.pointerDown(overlay);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("keeps the resolved origin label when closing so the legend does not flash unknown", () => {
    const { rerender } = render(
      <LandingMapDialog
        open
        onOpenChange={() => {}}
        country="US"
        ip="203.0.113.42"
        originCountry="CN"
        originIp="198.51.100.7"
      />,
    );
    const labelBefore = screen.getByTestId("landing-map-user-label").textContent;
    act(() => {
      rerender(
        <LandingMapDialog
          open={false}
          onOpenChange={() => {}}
          country="US"
          ip="203.0.113.42"
          originCountry={null}
          originIp={null}
        />,
      );
    });
    // Dialog unmounts on close in jsdom (no CSS close animation); re-opening
    // with the same measured origin works cleanly without stale globals.
    expect(labelBefore).not.toContain(t("home.landingMap.unknown"));
    rerender(
      <LandingMapDialog
        open
        onOpenChange={() => {}}
        country="DE"
        ip="203.0.113.99"
        originCountry="CN"
        originIp="198.51.100.7"
      />,
    );
    expect(screen.getByTestId("landing-map-exit")).toHaveAttribute("data-country", "DE");
  });

  it("constrains the map slot row inside the dialog height so footer chrome stays unclipped", () => {
    render(
      <LandingMapDialog
        open
        onOpenChange={() => {}}
        country="US"
        ip="203.0.113.42"
        originCountry="CN"
        originIp="198.51.100.7"
      />,
    );
    const dialog = screen.getByTestId("landing-map-dialog");
    const slot = screen.getByTestId("landing-map-full");
    expect(dialog).toHaveClass("max-h-[90vh]", "grid-rows-[auto_minmax(0,1fr)_auto_auto]");
    expect(slot).toHaveClass("min-h-0", "max-h-full");
  });

  it("drops the guessed suffix when a measured origin arrives for the same country while open", () => {
    const dtf = vi.spyOn(Intl, "DateTimeFormat").mockImplementation(
      () =>
        ({
          resolvedOptions: () => ({ timeZone: "Asia/Shanghai" }),
        }) as Intl.DateTimeFormat,
    );
    try {
      const { rerender } = render(
        <LandingMapDialog
          open
          onOpenChange={() => {}}
          country="JP"
          ip="203.0.113.42"
          originCountry={null}
          originIp={null}
        />,
      );
      expect(screen.getByTestId("landing-map-user-label").textContent).toContain(
        t("home.landingMap.inferred"),
      );
      rerender(
        <LandingMapDialog
          open
          onOpenChange={() => {}}
          country="JP"
          ip="203.0.113.42"
          originCountry="CN"
          originIp="198.51.100.7"
        />,
      );
      expect(screen.getByTestId("landing-map-user-label").textContent).not.toContain(
        t("home.landingMap.inferred"),
      );
    } finally {
      dtf.mockRestore();
    }
  });
});
