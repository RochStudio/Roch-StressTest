"""The registry: every tool this program knows how to start, in tab order.

Order is deliberate. The two CPU tests come first because they are what you
reach for after a clock change, then the two memory tests, then the two
Linpacks -- the harshest, and the ones most likely to be run last once
everything else has passed. Cinebench follows as a load rather than a check,
and the two graphics tools come last because they test a different piece of
hardware entirely.
"""

import os
import sys

from tool_3dmark11 import ThreeDMark11
from tool_cinebench import Cinebench
from tool_linpack import LinpackExtended, LinpackXtreme
from tool_memtest_vulkan import MemtestVulkan
from tool_occt import OCCT
from tool_prime95 import Prime95
from tool_ramtest import RamTestPro
from tool_testmem5 import TestMem5
from tool_ycruncher import YCruncher

TOOLS = (
    Prime95(),
    YCruncher(),
    TestMem5(),
    RamTestPro(),
    LinpackXtreme(),
    LinpackExtended(),
    # After the Linpacks because it is the same kind of thing done from one
    # window -- CPU, memory and the two together, each checking its answers.
    OCCT(),
    Cinebench(),
    MemtestVulkan(),
    ThreeDMark11(),
)

BY_KEY = {tool.key: tool for tool in TOOLS}


# Quick Start's two columns, written out rather than split down the middle of
# the tab order, so related tools sit together and stay where they were put.
# Anything added to TOOLS and not named here still appears: it goes to
# whichever column is shorter, so a new tool is never simply invisible.
QUICK_COLUMNS = (
    ("cinebench", "prime95", "linpack_extended", "linpack_xtreme", "occt"),
    ("ycruncher", "testmem5", "ramtest", "3dmark11", "memtest_vulkan"),
)


def quick_columns():
    """The tools for each Quick Start column, in the order they are shown."""
    columns = [[BY_KEY[key] for key in side if key in BY_KEY]
               for side in QUICK_COLUMNS]
    placed = {tool.key for side in columns for tool in side}
    for tool in TOOLS:
        if tool.key not in placed:
            columns[0 if len(columns[0]) <= len(columns[1]) else 1].append(tool)
    return columns


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
