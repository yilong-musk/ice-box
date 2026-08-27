import { describe, expect, it } from "vitest";
import {
  clearNodesSnapshot,
  delayTestTagsForGroup,
  delayTestTagsForList,
  delayResultTone,
  formatDelay,
  isGroupType,
  nodesEqual,
  readNodesSnapshot,
  resolveSelectedTag,
  writeNodesSnapshot,
} from "./nodes";

describe("formatDelay", () => {
  it("renders known states", () => {
    expect(formatDelay(null)).toBe("—");
    expect(formatDelay("testing")).toBe("…");
    expect(formatDelay("error")).toBe("失败");
    expect(formatDelay(42)).toBe("42 ms");
  });
});

describe("isGroupType", () => {
  it("recognizes strategy groups", () => {
    expect(isGroupType("selector")).toBe(true);
    expect(isGroupType("socks")).toBe(false);
  });
});

describe("delayResultTone", () => {
  it("uses green below 300ms, yellow through 999ms, red from 1000ms", () => {
    expect(delayResultTone(0)).toBe("ok");
    expect(delayResultTone(299)).toBe("ok");
    expect(delayResultTone(300)).toBe("warn");
    expect(delayResultTone(999)).toBe("warn");
    expect(delayResultTone(1000)).toBe("bad");
  });
});

describe("delayTestTagsForGroup", () => {
  it("tests only the selected exit when collapsed", () => {
    expect(
      delayTestTagsForGroup({
        expanded: false,
        groupNow: "node-a",
        groupAll: ["node-a", "node-b"],
      }),
    ).toEqual(["node-a"]);
  });

  it("returns no tags when collapsed without a selected exit", () => {
    expect(
      delayTestTagsForGroup({
        expanded: false,
        groupNow: null,
        groupAll: ["node-a", "node-b"],
      }),
    ).toEqual([]);
  });

  it("tests every member when expanded", () => {
    expect(
      delayTestTagsForGroup({
        expanded: true,
        groupNow: "node-a",
        groupAll: ["node-a", "node-b"],
      }),
    ).toEqual(["node-a", "node-b"]);
  });
});

describe("delayTestTagsForList", () => {
  const nodes = [
    { tag: "node-a", outbound_type: "socks", group_now: null, group_all: null },
    { tag: "node-b", outbound_type: "vmess", group_now: null, group_all: null },
    {
      tag: "选择组",
      outbound_type: "selector",
      group_now: "node-a",
      group_all: ["node-a", "node-b"],
    },
    {
      tag: "自动组",
      outbound_type: "urltest",
      group_now: "node-b",
      group_all: ["node-a", "node-b"],
    },
  ];

  it("dedupes leaf tags and does not probe group tags when collapsed", () => {
    expect(delayTestTagsForList(nodes, new Set())).toEqual(["node-a", "node-b"]);
  });

  it("includes every member of an expanded group once", () => {
    expect(delayTestTagsForList(nodes, new Set(["选择组"]))).toEqual([
      "node-a",
      "node-b",
    ]);
  });

  it("returns no tags when only collapsed groups lack an exit", () => {
    expect(
      delayTestTagsForList(
        [
          {
            tag: "选择组",
            outbound_type: "selector",
            group_now: null,
            group_all: ["node-a"],
          },
        ],
        new Set(),
      ),
    ).toEqual([]);
  });
});

describe("resolveSelectedTag", () => {
  it("keeps valid settings tag", () => {
    const nodes = [{ tag: "a" }, { tag: "b" }];
    expect(resolveSelectedTag("b", nodes)).toBe("b");
  });

  it("falls back to first node when settings tag missing", () => {
    const nodes = [{ tag: "a" }, { tag: "b" }];
    expect(resolveSelectedTag(null, nodes)).toBe("a");
  });

  it("falls back when settings tag is stale", () => {
    const nodes = [{ tag: "a" }];
    expect(resolveSelectedTag("gone", nodes)).toBe("a");
  });
});

describe("nodes snapshot cache", () => {
  it("stores and clears the last node list", () => {
    clearNodesSnapshot();
    expect(readNodesSnapshot()).toBeUndefined();
    writeNodesSnapshot({
      nodes: [{ tag: "a", outbound_type: "socks", group_now: null, group_all: null }],
      selectedTag: "a",
      running: true,
    });
    expect(readNodesSnapshot()?.selectedTag).toBe("a");
    expect(readNodesSnapshot()?.running).toBe(true);
    clearNodesSnapshot();
    expect(readNodesSnapshot()).toBeUndefined();
  });

  it("compares node lists by tag, type, and group members", () => {
    const a = [
      { tag: "a", outbound_type: "socks", group_now: null, group_all: null },
      {
        tag: "组",
        outbound_type: "selector",
        group_now: "a",
        group_all: ["a", "b"],
      },
    ];
    expect(nodesEqual(a, a)).toBe(true);
    expect(nodesEqual(a, structuredClone(a))).toBe(true);
    expect(
      nodesEqual(a, [
        a[0],
        { ...a[1], group_now: "b" },
      ]),
    ).toBe(false);
  });
});
