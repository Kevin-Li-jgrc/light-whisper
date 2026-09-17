import importlib.metadata
import unittest
from unittest import mock

from test_build_engine_atomicity import load_build_engine_module


class CudaRuntimePackagingTests(unittest.TestCase):
    def test_existing_provider_still_installs_missing_runtime_dependencies(self):
        module = load_build_engine_module()
        versions = {"transcribe-cpp-native-cu12": "0.1.3"}

        def version(name):
            if name not in versions:
                raise importlib.metadata.PackageNotFoundError(name)
            return versions[name]

        def install(command, **kwargs):
            for requirement in command:
                if requirement.startswith("nvidia-") and "==" in requirement:
                    name, value = requirement.split("==")
                    versions[name] = value

        with (
            mock.patch.object(module.importlib.metadata, "version", side_effect=version),
            mock.patch.object(module.shutil, "which", return_value="uv"),
            mock.patch.object(module.subprocess, "run", side_effect=install) as run,
        ):
            module.ensure_qwen3_cuda_provider()

        run.assert_called_once()
        self.assertIn("nvidia-cuda-runtime-cu12", versions)
        self.assertIn("nvidia-cublas-cu12", versions)


if __name__ == "__main__":
    unittest.main()
