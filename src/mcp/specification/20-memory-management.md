# 20. Memory Management

> **Part of the Clean Language Specification.** See [README](./README.md) for the full index.


Clean manages memory for you. You never allocate or free memory by hand, and there is no garbage collector and no reference counting.

### How Memory Is Used

- **Plain values** — `integer`, `number`, `boolean` — live in variables directly.
- **Strings, lists, pairs, matrices and objects** live in the program's memory area (the heap). A variable holds a reference to them.
- **Allocation is a bump.** Each new string, list or object takes the next free space at the end of the heap. Making one is fast.
- **Nothing is freed one value at a time.** A value that is no longer used keeps its space until the memory it lives in is reclaimed as a whole.

### When Memory Is Reclaimed

Memory is reclaimed in whole regions, not value by value:

- **Per request or event.** A host that runs the program for many requests (a server, a browser page) marks the heap when a request or event starts and resets it when the request or event ends. Everything made while handling it is reclaimed at once.
- **Inside loops.** In a loop that builds up a value, the compiler keeps the short-lived text of each step (for example the pieces joined with `+`) in a small reusable area, so the loop does not keep growing the heap with values it has already dropped.
- **At exit.** When a program run ends, all of its memory goes away.

A long-running `start:` that keeps making new strings or lists grows the heap until the run ends. Build big results once (for example with `list.join`) instead of rebuilding a string in every step of a very long loop.

```clean
start:
	list<string> parts = []
	iterate i in 1 to 5
		parts.add(i.toString())
	print(list.join(parts, ","))     // 1,2,3,4,5
```

### What This Means for Your Code

- You never write memory code: there is no `free`, `delete` or `dispose`.
- Values are never freed while you can still reach them.
- There are no reference counts, so circular references between objects are fine.

The exact layout of the heap and the functions a host uses to mark and reset it are in `foundation/spec/platform/MEMORY_MODEL.md`.

---

[← Previous: Asynchronous Programming](./19-async.md) | [Index](./README.md) | [Next: AI Integration](./21-ai-integration.md) →
