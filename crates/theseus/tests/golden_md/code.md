```rust
use std::collections::HashMap;

/// A reading of the tide gauge.
#[derive(Debug, Clone)]
pub struct Reading<'a> { name: &'a str, height: f64 }

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut seen: HashMap<String, u32> = HashMap::new(); // counts
    let line = "12:00 0.60\n";
    for (i, r) in line.split(' ').enumerate() {
        println!("{i}: {r:?} {}", 0x1F + 2_000);
    }
    /* block
       comment */
    Ok(())
}
```

```python
import json

@dataclass
class Reading:
    """A reading
    over two lines."""
    height: float = 0.6

def high_waters(readings: list[Reading]) -> int:
    # count the turning points
    return sum(1 for r in readings if r.height > 3.5 and not None)
```

```js
const readings = [1.5, 2, 0x10];
async function load(url) {
  const res = await fetch(`${url}/tides`); // network
  return res.json();
}
```

```ts
interface Reading { height: number; label?: string }
export const high = (r: Reading): boolean => r.height > 3.5;
```

```sh
set -e
for f in tides/*.txt; do
  echo "reading $f" # say it
  python3 -m unittest -v "$f" || exit 1
done
echo ${HOME:-/tmp} 'single'
```

```json
{"station": "Wenlow", "heights": [4.2, 0.6, -1.5e2], "ok": true, "note": null}
```

```toml
[package]
name = "tides"   # the crate
version = "0.1.0"
publish = false
```

```yaml
- name: build
  on: [push]
  steps:
    - run: cargo build  # compile
      retries: 3
      verbose: yes
```

```go
package main

func main() {
	s := `raw
string`
	var n int = 42
	fmt.Println("hi", 'x', n) // done
}
```

```c
#include <stdio.h>
/* a
   comment */
int main(void) {
    printf("hi %d\n", 42);
    return 0;
}
```

```cpp
#include <vector>
template <typename T> class Box { public: T value; };
int main() { std::vector<int> v{1, 2}; return v.size() > 1 ? 0 : 1; }
```

```diff
diff --git a/tides.py b/tides.py
--- a/tides.py
+++ b/tides.py
@@ -1,3 +1,3 @@
 def high(x):
-    return x > 3
+    return x > 3.5
```

```brainfuck
++++++++[>++++[>++>+++>+++>+<<<<-]>+>+>->>+[<]<-]
```

```
a fence with no language keeps its text as it is
    and its indentation
```

~~~text
a tilde fence
~~~

```rust
fn a_very_long_line_that_goes_past_the_edge_of_an_eighty_column_terminal(argument_one: &str, argument_two: &str, argument_three: u64) -> String { format!("{argument_one}{argument_two}{argument_three}") }
```

```rust
// a fence that is never closed
let x = 1;
