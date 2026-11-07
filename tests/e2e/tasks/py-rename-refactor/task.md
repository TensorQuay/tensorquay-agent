Rename the function `calc_total` to `compute_total` everywhere in this repository: its definition, every caller and
the tests. Keep the behaviour identical. `python3 -m shop.cli` must still print the same invoice, and
`python3 -m unittest discover -s tests -t .` must still pass.
