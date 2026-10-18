import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { TreeNode } from "../lib/api";
import { Meter } from "./RecordDock";
import { Tree, type TreeActions } from "./Tree";

const nodes: TreeNode[] = [
  {
    name: "Sales",
    path: "Sales",
    kind: "topic",
    modified: 1,
    children: [{ name: "Standup", path: "Sales/Standup.md", kind: "note", modified: 2 }],
  },
  { name: "Welcome", path: "Welcome.md", kind: "note", modified: 3 },
];

function actions(): TreeActions {
  return { open: vi.fn(), menu: vi.fn(), addMenu: vi.fn(), rename: vi.fn(), remove: vi.fn(), drop: vi.fn() };
}

describe("Tree", () => {
  it("shows children only when expanded, with tree semantics", () => {
    const { rerender } = render(<Tree nodes={nodes} selected={null} expanded={new Set()} onToggle={vi.fn()} recording={null} actions={actions()} />);
    expect(screen.getByRole("tree")).toBeTruthy();
    expect(screen.queryByText("Standup")).toBeNull();
    rerender(<Tree nodes={nodes} selected="Sales/Standup.md" expanded={new Set(["Sales"])} onToggle={vi.fn()} recording={null} actions={actions()} />);
    const item = screen.getByText("Standup").closest('[role="treeitem"]')!;
    expect(item.getAttribute("aria-selected")).toBe("true");
    expect(item.getAttribute("aria-level")).toBe("2");
  });

  it("opens with Enter, renames with F2 and expands with the right arrow", () => {
    const a = actions();
    const onToggle = vi.fn();
    render(<Tree nodes={nodes} selected={null} expanded={new Set()} onToggle={onToggle} recording={null} actions={a} />);
    const sales = screen.getByText("Sales").closest('[role="treeitem"]')!;
    fireEvent.keyDown(sales, { key: "ArrowRight" });
    expect(onToggle).toHaveBeenCalledWith("Sales", true);
    fireEvent.keyDown(sales, { key: "F2" });
    expect(a.rename).toHaveBeenCalledWith(nodes[0]);
    const welcome = screen.getByText("Welcome").closest('[role="treeitem"]')!;
    fireEvent.keyDown(welcome, { key: "Enter" });
    expect(a.open).toHaveBeenCalledWith(nodes[1]);
  });

  it("marks the note being recorded", () => {
    const { container } = render(<Tree nodes={nodes} selected={null} expanded={new Set()} onToggle={vi.fn()} recording="Welcome.md" actions={actions()} />);
    expect(container.querySelector(".rec-dot")).not.toBeNull();
  });
});

describe("Meter", () => {
  it("grows with the level", () => {
    const { container, rerender } = render(<Meter level={0} />);
    const quiet = (container.querySelector(".meter-bar") as HTMLElement).style.transform;
    rerender(<Meter level={1} />);
    const loud = (container.querySelector(".meter-bar") as HTMLElement).style.transform;
    expect(quiet).not.toBe(loud);
  });
});
