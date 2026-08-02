# Module System Example

This directory demonstrates Que's module system (spec §30).

## Quick Start

From this directory:

```sh
que main.que
```

## What's Inside

```
main.que             # Entry point — imports and uses all modules
utils.que            # Simple utility module (pub fn)
config.que           # Configuration module (pub fn values)
lib/
  mod.que            # Directory module entry point (pub import re-exports)
  math.que           # Math utilities (demonstrates caching)
  strings.que        # String utilities
```

## Features Demonstrated

| Feature | Where |
|---------|-------|
| Local imports (`import .utils`) | main.que |
| Nested paths (`import .lib.math`) | main.que |
| Aliased imports (`as str_utils`) | main.que |
| Selective imports (`{ add, mul }`) | main.que |
| Multi-module shorthand (`import .{utils, config}`) | main.que |
| Directory modules (`mod.que`) | lib/mod.que |
| `pub import` re-exports | lib/mod.que |
| `pub fn` visibility | utils.que, lib/math.que |
| Private functions (no `pub`) | utils.que, lib/math.que |
| Module caching (single eval) | lib/math.que prints once |
| Modules as Maps (`typeof`, `keys`) | main.que §5 |
| Standard library (`std.fs`, `std.json`) | main.que §7 |

## Module Resolution

Import paths map to files relative to the project root:

| Import | Resolves to |
|--------|-------------|
| `import .utils` | `utils.que` |
| `import .config` | `config.que` |
| `import .lib` | `lib/mod.que` |
| `import .lib.math` | `lib/math.que` |
| `import .lib.strings` | `lib/strings.que` |
| `import std.fs` | Built-in standard library |

## Expected Output

```
  [loader] Loading math module...
  [loader] Loading strings module...
═══ Module System Demo ═══

── 1. Namespace imports ──
  utils.greet("Que") → Hello, Que!
  utils.slug("Hello World") → hello-world
  math.add(2, 3) → 5
  math.factorial(6) → 720
  str_utils.capitalize("que") → Que

── 2. Selective imports ──
  add(10, 20) → 30
  mul(6, 7) → 42

── 3. Directory module (lib/mod.que) ──
  lib.version → 1.0.0
  lib.math_version → math v1.0
  lib.strings_version → strings v1.0

── 4. Config module ──
  config.app_name → Que Module Demo
  ...

── 5. Modules as Maps ──
  typeof(utils) → Map
  typeof(math) → Map
  ...

── 6. Module caching ──
  math module was loaded only once (check output above)

── 7. Standard library ──
  typeof(fs) → Map
  ...

═══ Done! ═══
```
