`sh build.sh` should create `dist/site.txt`: the line `Site build` followed by every page in `src/`, in the numeric
order of the page prefixes (1, 2, 3, 10, 11). It currently fails, and when it did run it put the pages in the wrong
order. Fix `build.sh`. Do not change the pages.
