# Copyright (c) Microsoft Corporation. All rights reserved.
# Licensed under the MIT License.

import json
import sys

obj = {}
obj["versionInfo"] = tuple(sys.version_info)
obj["sysPrefix"] = sys.prefix
# obj["sysVersion"] = sys.version
obj["sysVersion"] = "{}.{}.{}".format(sys.version_info.major, sys.version_info.minor, sys.version_info.micro)
obj["is64Bit"] = sys.maxsize > 2**32
obj["executable"] = sys.executable

# Everything after this is the information we need
print("503bebe7-c838-4cea-a1bc-0f2963bcb657")
print(json.dumps(obj))
