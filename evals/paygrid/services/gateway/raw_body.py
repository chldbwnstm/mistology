"""Raw HTTP body framing for gateway ingress."""


def extract_wire_body(http_frame: bytes, preserve_wire_bytes: bool = True) -> bytes:
    """Extract HTTP body after CRLFCRLF header boundary without mutating body bytes."""
    sep = b"\r\n\r\n"
    idx = http_frame.find(sep)
    body = http_frame if idx == -1 else http_frame[idx + len(sep) :]
    if not preserve_wire_bytes:
        return body.rstrip(b"\r\n")
    return body
