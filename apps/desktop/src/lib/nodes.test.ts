// SPDX-License-Identifier: GPL-3.0-or-later

import { beforeEach, describe, expect, it } from "vitest";
import { t } from "./i18n";
import {
  applyFavoriteToNodes,
  applyGroupNowToNodes,
  applySelectedTagToNodes,
  clearNodesSnapshot,
  delayTestTagsForGroup,
  delayTestTagsForList,
  delayResultTone,
  formatDelay,
  groupLineupKey,
  hasStrategyGroups,
  isFavoriteNode,
  isGroupType,
  nodesEqual,
  nodesSnapshotRevision,
  orderMembersByFavorite,
  orderNodesByFavorite,
  readExpandedGroups,
  readNodesSnapshot,
  reuseUnchangedNodes,
  resolveExitTag,
  resolveSelectedTag,
  sameNodeLineup,
  structuredTopLevel,
  subscribeNodesSnapshot,
  withoutGroupMembers,
  writeExpandedGroups,
  writeNodesSnapshot,
} from "./nodes";

describe("formatDelay", () => {
  it("renders known states", () => {
    expect(formatDelay(null)).toBe("");
    expect(formatDelay("testing")).toBe("…");
    expect(formatDelay("error")).toBe(t("delay.failed"));
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

  it("notifies subscribers on write and clear", () => {
    clearNodesSnapshot();
    const seen: Array<string | undefined> = [];
    const unsubscribe = subscribeNodesSnapshot((snap) => {
      seen.push(snap?.selectedTag);
    });
    writeNodesSnapshot({
      nodes: [{ tag: "a", outbound_type: "socks", group_now: null, group_all: null }],
      selectedTag: "a",
      running: false,
    });
    const revision = nodesSnapshotRevision();
    writeNodesSnapshot({
      nodes: [{ tag: "a", outbound_type: "socks", group_now: null, group_all: null }],
      selectedTag: "a",
      running: false,
    });
    expect(nodesSnapshotRevision()).toBe(revision);
    clearNodesSnapshot();
    unsubscribe();
    writeNodesSnapshot({
      nodes: [{ tag: "b", outbound_type: "vmess", group_now: null, group_all: null }],
      selectedTag: "b",
      running: false,
    });
    expect(seen).toEqual(["a", undefined]);
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

describe("applySelectedTagToNodes", () => {
  const nodes = [
    { tag: "a", outbound_type: "socks", group_now: null, group_all: null },
    {
      tag: "组",
      outbound_type: "selector",
      group_now: "a",
      group_all: ["a", "b"],
    },
  ];

  it("updates group_now when the picked leaf is a member", () => {
    expect(applySelectedTagToNodes(nodes, "b")[1]?.group_now).toBe("b");
  });

  it("returns the same array when the exit is already selected", () => {
    expect(applySelectedTagToNodes(nodes, "a")).toBe(nodes);
  });
});

describe("applyGroupNowToNodes", () => {
  const nodes = [
    {
      tag: "组",
      outbound_type: "selector",
      group_now: "a",
      group_all: ["a", "b"],
    },
  ];

  it("updates the named group's live exit", () => {
    expect(applyGroupNowToNodes(nodes, "组", "b")[0]?.group_now).toBe("b");
  });
});

describe("node favorites", () => {
  const node = (tag: string, favorite?: boolean, favorite_rank?: number | null) => ({
    tag,
    outbound_type: "socks",
    group_now: null,
    group_all: null,
    ...(favorite === undefined ? {} : { favorite }),
    ...(favorite_rank === undefined ? {} : { favorite_rank }),
  });

  it("orders favorites first by rank and keeps provider order otherwise", () => {
    const nodes = [
      node("a"),
      node("b", true, 1),
      node("c"),
      node("d", true, 0),
      node("e", true),
    ];
    expect(orderNodesByFavorite(nodes).map((item) => item.tag)).toEqual([
      "d",
      "b",
      "e",
      "a",
      "c",
    ]);
    expect(isFavoriteNode(node("e", true))).toBe(true);
    expect(isFavoriteNode(node("e", false, null))).toBe(false);
  });

  it("returns the original array when nothing is favorited", () => {
    const nodes = [node("a"), node("b")];
    expect(orderNodesByFavorite(nodes)).toBe(nodes);
    expect(orderNodesByFavorite([node("a", false, null)])[0]?.tag).toBe("a");
  });

  it("favoriting moves to the front, shifts ranks, and does not mutate input", () => {
    const nodes = [node("a", true, 0), node("b", true, 1), node("c")];
    const before = structuredClone(nodes);
    const next = applyFavoriteToNodes(nodes, "c", true);
    expect(next).not.toBe(nodes);
    expect(nodes).toEqual(before);
    expect(orderNodesByFavorite(next).map((item) => item.tag)).toEqual([
      "c",
      "a",
      "b",
    ]);
    expect(next.find((item) => item.tag === "c")?.favorite_rank).toBe(0);
    expect(next.find((item) => item.tag === "a")?.favorite_rank).toBe(1);
    expect(next.find((item) => item.tag === "b")?.favorite_rank).toBe(2);
  });

  it("unfavoriting closes the rank gap", () => {
    const nodes = [node("a", true, 0), node("b", true, 1), node("c", true, 2)];
    const next = applyFavoriteToNodes(nodes, "b", false);
    expect(orderNodesByFavorite(next).map((item) => item.tag)).toEqual([
      "a",
      "c",
      "b",
    ]);
    expect(next.find((item) => item.tag === "b")?.favorite).toBe(false);
    expect(next.find((item) => item.tag === "b")?.favorite_rank).toBeNull();
    expect(next.find((item) => item.tag === "c")?.favorite_rank).toBe(1);
  });

  it("compares normalized favorite fields", () => {
    const base = [node("a")];
    expect(nodesEqual(base, [node("a", false, null)])).toBe(true);
    expect(nodesEqual(base, [node("a", true, 0)])).toBe(false);
    expect(nodesEqual([node("a", true, 0)], [node("a", true, 1)])).toBe(false);
  });

  it("advances the snapshot revision only for selection changes", () => {
    clearNodesSnapshot();
    writeNodesSnapshot({
      nodes: [node("a")],
      selectedTag: "a",
      running: true,
    });
    const revision = nodesSnapshotRevision();
    writeNodesSnapshot({
      nodes: [node("a", true, 0)],
      selectedTag: "a",
      running: true,
    });
    expect(nodesSnapshotRevision()).toBe(revision);
    writeNodesSnapshot({
      nodes: [node("a", true, 1)],
      selectedTag: "a",
      running: false,
    });
    expect(nodesSnapshotRevision()).toBe(revision);
    writeNodesSnapshot({
      nodes: [node("b")],
      selectedTag: "b",
      running: false,
    });
    expect(nodesSnapshotRevision()).toBe(revision + 1);
    clearNodesSnapshot();
    expect(nodesSnapshotRevision()).toBe(revision + 2);
  });
});

describe("reuseUnchangedNodes", () => {
  const leaf = (tag: string) => ({
    tag,
    outbound_type: "trojan",
    group_now: null,
    group_all: null,
  });

  it("returns the previous list when every node is unchanged", () => {
    const prev = [leaf("a"), leaf("b")];
    expect(reuseUnchangedNodes(prev, [leaf("a"), leaf("b")])).toBe(prev);
  });

  it("keeps unchanged node objects and takes changed ones", () => {
    const group = {
      tag: "g",
      outbound_type: "selector",
      group_now: "a",
      group_all: null,
    };
    const prev = [group, leaf("a"), leaf("b")];
    const nextGroup = { ...group, group_all: ["a", "b"] };
    const merged = reuseUnchangedNodes(prev, [nextGroup, leaf("a"), leaf("b")]);
    expect(merged).not.toBe(prev);
    expect(merged[0]).toBe(nextGroup);
    expect(merged[1]).toBe(prev[1]);
    expect(merged[2]).toBe(prev[2]);
  });

  it("follows the new order and drops removed nodes", () => {
    const prev = [leaf("a"), leaf("b"), leaf("c")];
    const merged = reuseUnchangedNodes(prev, [leaf("c"), leaf("a")]);
    expect(merged.map((node) => node.tag)).toEqual(["c", "a"]);
    expect(merged[0]).toBe(prev[2]);
    expect(merged[1]).toBe(prev[0]);
  });

  it("takes a favorite change", () => {
    const prev = [leaf("a")];
    const next = [{ ...leaf("a"), favorite: true, favorite_rank: 0 }];
    expect(reuseUnchangedNodes(prev, next)[0]).toBe(next[0]);
  });
});

describe("snapshot member lists", () => {
  it("stores the list without member lists", () => {
    clearNodesSnapshot();
    const leafNode = {
      tag: "a",
      outbound_type: "socks",
      group_now: null,
      group_all: null,
    };
    writeNodesSnapshot({
      nodes: [
        {
          tag: "组",
          outbound_type: "selector",
          group_now: "a",
          group_all: ["a", "b"],
        },
        leafNode,
      ],
      selectedTag: "a",
      running: true,
    });
    const snap = readNodesSnapshot();
    expect(snap?.nodes[0]?.group_all).toBeNull();
    expect(snap?.nodes[0]?.group_now).toBe("a");
    expect(snap?.nodes[1]).toBe(leafNode);
    clearNodesSnapshot();
  });

  it("returns the same list when there is nothing to drop", () => {
    const nodes = [
      { tag: "a", outbound_type: "socks", group_now: null, group_all: null },
    ];
    expect(withoutGroupMembers(nodes)).toBe(nodes);
  });
});

describe("sameNodeLineup", () => {
  const leaf = (tag: string, type = "trojan") => ({
    tag,
    outbound_type: type,
    group_now: null,
    group_all: null,
  });
  const group = (tag: string, members: string[] | null, now = "a") => ({
    tag,
    outbound_type: "selector",
    group_now: now,
    group_all: members,
  });

  it("ignores order, group exits and favorites", () => {
    expect(
      sameNodeLineup(
        [group("g", ["a", "b"]), leaf("a"), leaf("b")],
        [
          { ...leaf("b"), favorite: true, favorite_rank: 0 },
          group("g", ["a", "b"], "b"),
          leaf("a"),
        ],
      ),
    ).toBe(true);
  });

  it("matches a group whose members are unknown on one side", () => {
    expect(sameNodeLineup([group("g", null), leaf("a")], [group("g", ["a"]), leaf("a")])).toBe(
      true,
    );
  });

  it("detects another set of tags, types or members", () => {
    expect(sameNodeLineup([leaf("a")], [leaf("b")])).toBe(false);
    expect(sameNodeLineup([leaf("a")], [leaf("a"), leaf("b")])).toBe(false);
    expect(sameNodeLineup([leaf("a")], [leaf("a", "vmess")])).toBe(false);
    expect(
      sameNodeLineup(
        [group("g", ["a", "b"]), leaf("a")],
        [group("g", ["a", "c"]), leaf("a")],
      ),
    ).toBe(false);
  });
});

describe("profile structure", () => {
  const leaf = (tag: string, favorite_rank: number | null = null) => ({
    tag,
    outbound_type: "vless",
    group_now: null,
    group_all: null,
    favorite: favorite_rank != null,
    favorite_rank,
  });
  const group = (tag: string, members: string[] | null) => ({
    tag,
    outbound_type: "selector",
    group_now: null,
    group_all: members,
  });

  it("detects strategy groups", () => {
    expect(hasStrategyGroups([leaf("a")])).toBe(false);
    expect(hasStrategyGroups([leaf("a"), group("g", ["a"])])).toBe(true);
  });

  it("lists every node of a flat profile, favorites first", () => {
    const top = structuredTopLevel([leaf("a"), leaf("b", 0)]);
    expect(top.map((node) => node.tag)).toEqual(["b", "a"]);
  });

  it("lists groups in order, then standalone nodes with favorites first", () => {
    const top = structuredTopLevel([
      group("g1", ["a"]),
      group("g2", ["a", "b"]),
      leaf("a"),
      leaf("b"),
      leaf("c"),
      leaf("d", 0),
    ]);
    expect(top.map((node) => node.tag)).toEqual(["g1", "g2", "d", "c"]);
  });

  it("leaves standalone nodes out while group members are unknown", () => {
    const top = structuredTopLevel([group("g", null), leaf("a"), leaf("b")]);
    expect(top.map((node) => node.tag)).toEqual(["g"]);
  });

  it("orders favorited members first by rank", () => {
    const byTag = new Map(
      [leaf("a"), leaf("b", 1), leaf("c", 0)].map((node) => [node.tag, node]),
    );
    expect(orderMembersByFavorite(["a", "b", "c", "direct"], byTag)).toEqual([
      "c",
      "b",
      "a",
      "direct",
    ]);
    const members = ["a", "direct"];
    expect(orderMembersByFavorite(members, byTag)).toBe(members);
  });
});

describe("remembered expanded groups", () => {
  beforeEach(() => {
    window.localStorage.removeItem("ice-box.nodes.expanded-groups");
  });

  it("keys lineups by their sorted group tags and skips flat profiles", () => {
    expect(groupLineupKey([{ tag: "a", outbound_type: "vless" } as never])).toBe("");
    expect(
      groupLineupKey([
        { tag: "z", outbound_type: "urltest" },
        { tag: "a", outbound_type: "vless" },
        { tag: "m", outbound_type: "selector" },
      ] as never),
    ).toBe(JSON.stringify(["m", "z"]));
  });

  it("stores expanded groups per lineup", () => {
    expect(readExpandedGroups("one")).toEqual(new Set());
    writeExpandedGroups("one", new Set(["g1"]));
    writeExpandedGroups("two", new Set(["g2", "g3"]));
    expect(readExpandedGroups("one")).toEqual(new Set(["g1"]));
    expect(readExpandedGroups("two")).toEqual(new Set(["g2", "g3"]));
    writeExpandedGroups("", new Set(["ignored"]));
    expect(readExpandedGroups("")).toEqual(new Set());
  });

  it("keeps only the most recent lineups", () => {
    for (let index = 0; index < 10; index += 1) {
      writeExpandedGroups(`lineup-${index}`, new Set([`g${index}`]));
    }
    expect(readExpandedGroups("lineup-9")).toEqual(new Set(["g9"]));
    expect(readExpandedGroups("lineup-2")).toEqual(new Set(["g2"]));
    expect(readExpandedGroups("lineup-1")).toEqual(new Set());
  });

  it("ignores malformed storage", () => {
    window.localStorage.setItem("ice-box.nodes.expanded-groups", "{not json");
    expect(readExpandedGroups("one")).toEqual(new Set());
    window.localStorage.setItem(
      "ice-box.nodes.expanded-groups",
      JSON.stringify([{ lineup: "one", groups: [1] }]),
    );
    expect(readExpandedGroups("one")).toEqual(new Set());
  });
});

describe("resolveExitTag", () => {
  const byTag = new Map(
    [
      { tag: "main", outbound_type: "selector", group_now: "auto", group_all: ["auto", "a"] },
      { tag: "auto", outbound_type: "urltest", group_now: "a", group_all: ["a", "b"] },
      { tag: "idle", outbound_type: "selector", group_now: null, group_all: ["a"] },
      { tag: "loop-1", outbound_type: "selector", group_now: "loop-2", group_all: ["loop-2"] },
      { tag: "loop-2", outbound_type: "selector", group_now: "loop-1", group_all: ["loop-1"] },
      { tag: "a", outbound_type: "vless", group_now: null, group_all: null },
    ].map((node) => [node.tag, node]),
  );

  it("follows nested groups to the node traffic leaves by", () => {
    expect(resolveExitTag("main", byTag)).toBe("a");
    expect(resolveExitTag("auto", byTag)).toBe("a");
    expect(resolveExitTag("a", byTag)).toBe("a");
  });

  it("keeps a group without an exit and unknown tags as they are", () => {
    expect(resolveExitTag("idle", byTag)).toBe("idle");
    expect(resolveExitTag("direct", byTag)).toBe("direct");
  });

  it("stops on a cycle", () => {
    expect(["loop-1", "loop-2"]).toContain(resolveExitTag("loop-1", byTag));
  });
});
