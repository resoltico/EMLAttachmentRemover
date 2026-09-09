"""Prefix-indexed removal traversal for the MIME-pruned execution pipeline."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import TYPE_CHECKING

from .domain import AppError, ExitCode, MimePath

if TYPE_CHECKING:
    from .mime_raw import RawNode


@dataclass(frozen=True, slots=True)
class _RemovalTrie:
    """One immutable node in the removal-prefix index."""

    removal: bool
    children: dict[int, _RemovalTrie]


@dataclass(slots=True)
class _MutableRemovalTrie:
    """Construction-only counterpart of the immutable removal trie."""

    removal: bool = False
    children: dict[int, _MutableRemovalTrie] = field(default_factory=dict)


@dataclass(frozen=True, slots=True)
class RetainedTraversal:
    """The retained source order and exact bounded-work receipt for one traversal."""

    nodes: tuple[RawNode, ...]
    node_visits: int
    child_edge_lookups: int


@dataclass(frozen=True, slots=True)
class RemovalIndex:
    """A prefix trie that makes retained-node traversal one source-tree pass."""

    root: _RemovalTrie

    @classmethod
    def from_roots(cls, roots: set[MimePath]) -> RemovalIndex:
        """Build a closed removal-prefix trie, rejecting overlapping roots.

        Returns:
            An index that answers ancestor removal membership during one tree walk.

        Raises:
            AppError: If callers supplied duplicate or overlapping removal roots.

        """
        mutable = _MutableRemovalTrie()
        for path in sorted(roots):
            current = mutable
            for component in path:
                if current.removal:
                    raise AppError(
                        ExitCode.VERIFICATION_ERROR,
                        "overlapping MIME removal roots",
                    )
                current = current.children.setdefault(component, _MutableRemovalTrie())
            current.removal = True
        return cls(_freeze(mutable))

    def retained_nodes(self, root: RawNode) -> tuple[RawNode, ...]:
        """Return retained nodes in source order with no per-node prefix scans.

        Returns:
            Every node not below one removal root, in source order.

        """
        return self.traverse(root).nodes

    def traverse(self, root: RawNode) -> RetainedTraversal:
        """Return retained nodes and a deterministic linear-work receipt.

        Returns:
            Retained source order and exact node/edge operation counts.

        """
        result: list[RawNode] = []
        pending: list[tuple[RawNode, _RemovalTrie]] = [(root, self.root)]
        node_visits = 0
        child_edge_lookups = 0
        while pending:
            node, trie = pending.pop()
            node_visits += 1
            if trie.removal:
                continue
            result.append(node)
            child_edge_lookups += len(node.children)
            pending.extend(
                (child, trie.children.get(child.path[-1], _EMPTY))
                for child in reversed(node.children)
            )
        return RetainedTraversal(tuple(result), node_visits, child_edge_lookups)

    def changed_ancestor_paths(self, root: RawNode) -> frozenset[MimePath]:
        """Return retained ancestors of every removal root in one source-tree walk.

        Returns:
            Paths of containers whose bodies changed because a child was removed.

        """
        changed: set[MimePath] = set()
        pending: list[tuple[RawNode, _RemovalTrie, tuple[RawNode, ...]]] = [
            (root, self.root, ())
        ]
        while pending:
            node, trie, ancestors = pending.pop()
            if trie.removal:
                changed.update(parent.path for parent in ancestors)
                continue
            pending.extend(
                (
                    child,
                    trie.children.get(child.path[-1], _EMPTY),
                    (*ancestors, node),
                )
                for child in reversed(node.children)
            )
        return frozenset(changed)


def _freeze(mutable: _MutableRemovalTrie) -> _RemovalTrie:
    """Return the immutable form of a construction-only removal trie.

    Returns:
        The recursively frozen internal trie node.

    """
    return _RemovalTrie(
        removal=mutable.removal,
        children={key: _freeze(value) for key, value in mutable.children.items()},
    )


_EMPTY = _RemovalTrie(removal=False, children={})
