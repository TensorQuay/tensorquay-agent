"""Print a sample invoice."""
from shop.invoice import invoice


def main():
    print(invoice([("tea", 450, 2), ("cake", 325, 1)], 8))


if __name__ == "__main__":
    main()
