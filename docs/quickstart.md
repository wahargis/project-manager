# Quickstart

## Build

```bash
cargo build --release
```

Run `./target/release/pm --help` for the full command surface.

## Create a synthetic project

```bash
pm --db /tmp/atlas.db project activate atlas --alias at
pm --db /tmp/atlas.db phase atlas add "Map retrieval gaps" --impact 40
pm --db /tmp/atlas.db phase atlas add "Test topic briefings" --impact 80
pm --db /tmp/atlas.db phase atlas add "Package handoff" --impact 60 --depends 1
pm --db /tmp/atlas.db experiment add atlas 1 "Inventory current retrieval behavior"
pm --db /tmp/atlas.db finding add atlas 1 "Retrieval misses code-symbol queries"
pm --db /tmp/atlas.db decision add atlas 1 "Adopt trigram tokenizer" --why "Code symbols are not matched by the default tokenizer"
```

Then inspect:

```bash
pm --db /tmp/atlas.db phase list atlas
pm --db /tmp/atlas.db next atlas
pm --db /tmp/atlas.db dashboard atlas
```

## Start the dashboard

```bash
pm --db /tmp/atlas.db serve
```

Open the printed URL in a browser.
