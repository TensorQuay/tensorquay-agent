`listTasks` in `src/tasks.ts` can filter by tag. Add an optional `status` filter (`"open"` or `"done"`) to its options.
When both `tag` and `status` are given, a task must match both. Without `status`, the result must not change. Any
other status value must throw an `Error`. Add a test for the new filter under `test/`, and keep
`node --test test/*.test.ts` passing.
