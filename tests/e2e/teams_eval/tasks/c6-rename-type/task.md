Rename the type `Record` to `StockItem` everywhere in the package `inventory/` and its docs in `docs/`. The public
API must not break: `from inventory import Record` must keep working, as an alias of `StockItem`. Keep the behaviour
identical; the tests may move to the new name. Run the tests with `python3 -m unittest discover -s tests -t .`.
