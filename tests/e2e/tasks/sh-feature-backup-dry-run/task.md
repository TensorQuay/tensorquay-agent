Add a `--dry-run` option to `backup.sh`, given as the first argument: `sh backup.sh --dry-run SOURCE DEST` prints
`would copy <path>` for every file it would copy, in the same order as a real run, and changes nothing on disk: no
directory or file is created under DEST. Without the option the script must behave exactly as it does today.
