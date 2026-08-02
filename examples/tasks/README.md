# Task System Example

This directory demonstrates Que's task system with real file I/O.

## Quick Start

From this directory:

```sh
# List all available tasks
que tasks

# Run the full build pipeline
que run build

# Run it again — notice "bundle" and "transform" are skipped (outputs are fresh)
que run build

# Clean and rebuild from scratch
que run clean build

# Generate a full report (includes build + CSV parsing)
que run report

# Show project statistics
que run stats

# Try a parameterized task
que run greet    # uses defaults

# List all tasks with metadata
que run list_all
```

## What's Inside

```
Quefile             # Task definitions
src/
  greeting.txt       # Source file (bundle input)
  features.txt       # Source file (bundle input)
  credits.txt        # Source file (bundle input)
data/
  users.csv          # CSV data for parse_csv task
build/               # Created by tasks (gitignored)
```

## Task Graph

```
clean

prepare ──► bundle ──► transform ──► build ──┐
                                             ├──► report
prepare ──► parse_csv ───────────────────────┘

stats       (standalone)
greet       (standalone, parameterized)
list_all    (standalone, introspection)
```

## Showcased Features

1. **Dependencies** — `build` triggers `prepare → bundle → transform` automatically
2. **Diamond dedup** — `report` depends on both `build` and `parse_csv`, which
   both depend on `prepare`; it only runs once
3. **Input/output freshness** — Run `que run build` twice; the second time
   `bundle` and `transform` are skipped because outputs are newer than inputs
4. **Parameterized tasks** — `greet` accepts `name` and `greeting` with defaults
5. **Introspection** — `list_all` uses `tasks()` to enumerate all tasks at runtime
6. **Real file I/O** — Tasks read source files, write bundles, parse CSV, generate reports
