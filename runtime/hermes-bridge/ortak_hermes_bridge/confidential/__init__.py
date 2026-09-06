"""Isolated confidential codecs. No bridge, journal or worker route imports this package.

Validated claims and successful decryption never grant current authority. No
credential resolution, key wrapping, persistence or provider call lives here.
"""
