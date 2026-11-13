from __future__ import annotations


class PostalCodeFormatError(ValueError):
    pass


def validatePostalCode(code: str) -> str:
    if not isinstance(code, str):
        raise PostalCodeFormatError("postal code must be str")
    cleaned = code.strip()
    if len(cleaned) != 5 or not cleaned.isascii() or not cleaned.isdigit():
        raise PostalCodeFormatError(f"postal code {cleaned} must be 5 digits")
    return cleaned
