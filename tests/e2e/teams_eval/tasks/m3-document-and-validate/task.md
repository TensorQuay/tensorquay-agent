Document the service in `svc/` as `API.md`: every route, written as its method and path (for example
`GET /items/<id>`), with its request and its responses, from the code. Writing it down shows input validation the
code is missing against the rules in `svc/README.md`: add it, so an item that breaks them is refused with status
400 when created or updated. Run the tests with `python3 -m unittest discover -s tests -t .`.
