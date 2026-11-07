"""The reviews: create, read, list, update and delete, through store/."""
import store


def create_review(path, product_id, customer_id, stars):
    return store.open(path).reviews.insert({"product_id": product_id, "customer_id": customer_id, "stars": stars})


def get_review(path, review_id):
    return store.open(path).reviews.get(review_id)


def list_reviews(path):
    return store.open(path).reviews.all()


def find_reviews(path, product_id):
    return store.open(path).reviews.where("product_id", product_id)


def update_review(path, review_id, product_id, customer_id, stars):
    return store.open(path).reviews.update(review_id, {"product_id": product_id, "customer_id": customer_id, "stars": stars})


def delete_review(path, review_id):
    return store.open(path).reviews.delete(review_id)
