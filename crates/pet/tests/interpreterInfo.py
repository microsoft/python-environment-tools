# Copyright (c) Microsoft Corporation. All rights reserved.
# Licensed under the MIT License.

import json
import sys

obj = {}
obj["version_info"] = tuple(sys.version_info)
obj["sys_prefix"] = sys.prefix
obj["sys_version"] = sys.version
obj["is64_bit"] = sys.maxsize > 2**32
obj["executable"] = sys.executable

# Everything after this is the information we need
print("503bebe7-c838-4cea-a1bc-0f2963bcb657")
print(json.dumps(obj))
