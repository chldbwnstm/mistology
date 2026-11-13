from __future__ import annotations


class HeaderInjectionError(ValueError):
    pass


def sendReceiptEmail(recipient_name: str, email: str) -> dict[str, str]:
    if (
        "\r" in recipient_name
        or "\n" in recipient_name
        or "\r" in email
        or "\n" in email
    ):
        raise HeaderInjectionError("newline in recipient header")
    if "@" not in email:
        raise ValueError("invalid email address")
    return {
        "to": f"{recipient_name} <{email}>",
        "subject": "Your ShopCore Receipt",
    }
