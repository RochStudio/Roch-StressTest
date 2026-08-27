"""The registry: every tool this program knows how to start, in tab order.

Order is deliberate. The two CPU tests come first because they are what you
reach for after a clock change, then the two memory tests, then Linpack --
which is the harshest and the one most likely to be run last, once everything
else has passed.
"""

import os
import sys

from tool_linpack import Linpack
from tool_prime95 import Prime95
from tool_ramtest import RamTestPro
from tool_testmem5 import TestMem5
from tool_ycruncher import YCruncher

TOOLS = (
    Prime95(),
    YCruncher(),
    TestMem5(),
    RamTestPro(),
    Linpack(),
)

BY_KEY = {tool.key: tool for tool in TOOLS}


def tools_root():
    """The folder the tool distributions are unpacked into.

    Beside this file when running from source, and beside the executable when
    frozen -- PyInstaller unpacks the code to a temporary directory, so
    ``__file__`` points somewhere useless in a build.
    """
    if getattr(sys, "frozen", False):
        return os.path.dirname(sys.executable)
    return os.path.dirname(os.path.abspath(__file__))


def available(root=None):
    """The tools whose executables are actually present."""
    root = root or tools_root()
    return [tool for tool in TOOLS if tool.available(root)]


def missing(root=None):
    root = root or tools_root()
    return [tool for tool in TOOLS if not tool.available(root)]
