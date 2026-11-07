"""The coupons: create, read, list, update and delete, through store/."""
import store


def create_coupon(path, code, percent, active):
    return store.open(path).coupons.insert({"code": code, "percent": percent, "active": active})


def get_coupon(path, coupon_id):
    return store.open(path).coupons.get(coupon_id)


def list_coupons(path):
    return store.open(path).coupons.all()


def find_coupons(path, code):
    return store.open(path).coupons.where("code", code)


def update_coupon(path, coupon_id, code, percent, active):
    return store.open(path).coupons.update(coupon_id, {"code": code, "percent": percent, "active": active})


def delete_coupon(path, coupon_id):
    return store.open(path).coupons.delete(coupon_id)
