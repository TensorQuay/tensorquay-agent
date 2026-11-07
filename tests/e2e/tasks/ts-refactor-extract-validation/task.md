Move `isEmail` and `isHandle` out of `src/users.ts` into a new module `src/validate.ts`, which exports both. Make
`src/users.ts` import them from there instead of defining them. Behaviour must not change, and
`node --test test/*.test.ts` must still pass. Do not change the tests.
