import os
import sys

from setuptools import setup
from setuptools_rust import Binding, RustExtension

# Ensure these are present (in case we are not using PEP-518 compatible build
# system).
import setuptools_scm

def env_true(env):
    return env == '1'

noflags = env_true(os.environ.get("SCALIB_NOFLAGS"))
portable = not noflags and env_true(os.environ.get("SCALIB_PORTABLE"))
use_avx2 = not noflags and env_true(os.environ.get("SCALIB_AVX2"))

if portable and use_avx2:
    raise ValueError("Cannot have both SCALIB_PORTABLE and SCALIB_AVX2.")

# We check only for AVX2, as this is the CI default, otherwise we assume local
# builds.
with open("src/scalib/build_config.py", "w") as f:
    f.write(f"REQUIRE_AVX2 = {use_avx2}\n")


if noflags or portable:
    rustflags = None
elif use_avx2:
    rustflags = "-C target-feature=+avx2"
else:
    rustflags = "-C target-cpu=native"

if rustflags:
    rustflags = os.environ.get('RUSTFLAGS', '') + ' ' + rustflags
    os.environ["RUSTFLAGS"] = rustflags

print(f"Build config: {noflags=} {portable=} {use_avx2=} {rustflags=}.")

scalib_features = ["pyo3/abi3"]

if sys.platform == "linux":
    scalib_features.append("blis")

#告诉setuptools如何构建和集成Rust代码
setup(
    project_urls={
        "Bug Tracker": "https://github.com/simple-crypto/scalib/issues",
    },
    rust_extensions=[
        RustExtension(
            "scalib._scalib_ext",#Rust扩展的名称，Python代码中将使用这个名称来导入Rust编译的模块
            path="src/scalib_ext/scalib-py/Cargo.toml",#Python将会调用scalib_ext/scalib-py目录下的Rust代码
            binding=Binding.PyO3,#这指定了使用PyO3库为Rust和Python之间提供绑定。PyO3是一个让Rust和Python互操作的流行库
            features=scalib_features,
            py_limited_api=True,
        )
    ],
)
