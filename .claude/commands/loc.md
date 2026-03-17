Generate a file tree of the project with lines of code (LOC) counts for each file and folder.

Instructions:
1. Use `find src -name '*.rs' -type f` to get all Rust source files
2. Use `wc -l` on each file to get line counts
3. Display as an indented tree structure showing:
   - Each folder with its total LOC (sum of all files inside, recursively)
   - Each file with its LOC
4. Sort folders and files alphabetically within each level
5. Show the grand total at the bottom
6. Use tree-drawing characters (├── └── │) for the tree structure

Example output format:
```
src/ (14,300 loc)
├── main.rs (250 loc)
├── network.rs (180 loc)
├── tests/ (1,200 loc)
│   ├── operations.rs (800 loc)
│   └── mod.rs (400 loc)
└── ui/ (3,500 loc)
    ├── mod.rs (100 loc)
    └── components.rs (3,400 loc)
```

Only count `.rs` files. Format numbers with commas for readability. Present the result directly without extra commentary.
