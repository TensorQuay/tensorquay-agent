# Review against spec/LIMITER.md

| Rule | Verdict | Note |
|---|---|---|
| 1 separate keys, start full | ok | |
| 2 refill up to burst | ok | capped with min() |
| 3 a refusal takes nothing | ok | |
| 4 cost above burst never allowed | fixed | first version allowed it on a full bucket |
| 5 cost must be positive | fixed | first version took 0 and negatives |
| 6 clock set back adds nothing | fixed | first version added negative time |
| 7 remaining takes nothing | ok | |
