# Small string helpers for POSIX sh. Each prints its result without a trailing newline.

# trim TEXT: TEXT without leading or trailing spaces and tabs.
trim() {
    printf '%s' "$1" | sed -e 's/^[[:blank:]]*//' -e 's/[[:blank:]]*$//'
}

# upper TEXT: TEXT with ASCII letters in upper case.
upper() {
    printf '%s' "$1" | tr 'a-z' 'A-Z'
}

# repeat_char CHAR COUNT: CHAR repeated COUNT times; nothing when COUNT is 0.
repeat_char() {
    count=$2
    while [ "$count" -gt 1 ]; do
        printf '%s' "$1"
        count=$((count - 1))
    done
}
