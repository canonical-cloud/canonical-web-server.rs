#!/usr/bin/env python3
from itertools import product


def allowed(method: str, tenant_match: bool, mutates: bool) -> bool:
    if method == 'GET':
        return tenant_match and not mutates
    return tenant_match


def main() -> None:
    explored = 0
    for method, tenant_match, mutates in product(('GET', 'POST'), (False, True), (False, True)):
        explored += 1
        ok = allowed(method, tenant_match, mutates)
        if ok:
            assert tenant_match
            if method == 'GET':
                assert not mutates, 'read route mutated state'
    assert not allowed('GET', True, True)
    assert not allowed('GET', False, False)
    print(f'web read-path purity: {explored} states')


if __name__ == '__main__':
    main()
