"""Tests for :mod:`kimberlite.compliance` orchestrator-helper internals.

Pins the v0.8.0 arity-detection behaviour for the
``erase_subject(on_stream=...)`` callback so legacy 1-arg callbacks
keep working alongside the new ``(stream_id, request_id)`` shape.
"""

from kimberlite.compliance import _callback_accepts_request_id


def test_one_arg_callback_uses_legacy_shape():
    def legacy(stream_id):
        return 0

    assert _callback_accepts_request_id(legacy) is False


def test_two_arg_callback_accepts_request_id():
    def upgraded(stream_id, request_id):
        return 0

    assert _callback_accepts_request_id(upgraded) is True


def test_var_args_callback_treated_as_accepting():
    def flexible(*args):
        return 0

    assert _callback_accepts_request_id(flexible) is True


def test_keyword_only_request_id_not_recognised_as_positional():
    # Keyword-only args don't count toward the positional arity check —
    # callers who want the new shape must declare it positionally.
    def odd(stream_id, *, request_id=None):
        return 0

    assert _callback_accepts_request_id(odd) is False


def test_lambda_one_arg():
    assert _callback_accepts_request_id(lambda s: 0) is False


def test_lambda_two_arg():
    assert _callback_accepts_request_id(lambda s, r: 0) is True


def test_unintrospectable_callable_falls_back_to_legacy():
    # Builtins like ``len`` lack a Python-introspectable signature in
    # some interpreters; the helper must not raise.
    assert _callback_accepts_request_id(len) is False
