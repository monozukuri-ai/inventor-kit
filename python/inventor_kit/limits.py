"""Per-document resource ceilings, checked again by Rust at every entry point."""
from dataclasses import asdict, dataclass, fields
import json


@dataclass(frozen=True)
class Limits:
    """Tighten parser limits; zero denies a resource. Raising defaults is rejected.

    Byte/item ceilings are not process RSS or time guarantees. Assembly graph
    budgets remain FileSystemResolver options. OCCT allocations are separate.
    """
    max_file_bytes: int = 128 * 1024 * 1024
    max_stream_bytes: int = 64 * 1024 * 1024
    max_inflated_bytes: int = 64 * 1024 * 1024
    max_records: int = 500_000
    max_streams: int = 65536
    max_property_bytes: int = 16 * 1024 * 1024
    max_property_items: int = 100_000
    max_property_depth: int = 16
    max_total_inflated_bytes: int = 128 * 1024 * 1024
    max_candidates: int = 256

    def __post_init__(self):
        for field in fields(self):
            value = getattr(self, field.name)
            if type(value) is not int or not 0 <= value <= field.default:
                raise ValueError(f'{field.name} must be an integer from 0 to {field.default}')


def effective(limits):
    if limits is None:
        return Limits()
    if not isinstance(limits, Limits):
        raise TypeError('limits must be inventor_kit.Limits or None')
    limits.__post_init__()
    return limits


def encoded(limits):
    return json.dumps(asdict(effective(limits)))
