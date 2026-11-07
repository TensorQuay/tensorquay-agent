The package in `shop/` keeps money as floating point, and some of its tests fail on rounding. Change `Money` to hold
integer cents (as a `cents` attribute) everywhere in the package, so that no module does float arithmetic on money,
while keeping `Money`'s public API (the names in its docstring) and every module's function names and arguments.
`amount` may become an exact decimal. Don't change the tests; run them with `python3 -m unittest discover -s tests -t .`.
