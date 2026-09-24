import unittest
import os
import sys

# Add project root to sys.path
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "..")))

from config import ScannerConfig, ScannerModel
from drivers.driver_factory import create_driver_for_model, get_driver_class_for_model
from drivers.bcd436hp import BCD436HPDriver
from drivers.bcd996p2 import BCD996P2Driver
from drivers.classic_driver import UnidenClassicDriver

class TestDriverFactory(unittest.TestCase):

    def test_all_34_models_driver_mapping(self):
        """Verify that all 34 Uniden models map to a valid driver class without throwing exceptions"""
        for model in ScannerModel:
            cfg = ScannerConfig(id="test_scanner", name=f"Test {model.value}", model=model, port="COM1")
            driver_cls = get_driver_class_for_model(model)
            self.assertIsNotNone(driver_cls)

            # Test real driver instantiation
            driver = create_driver_for_model(cfg)
            self.assertIsNotNone(driver)
            self.assertEqual(driver.config.model, model)

    def test_specific_model_family_mappings(self):
        # SDS / HomePatrol family -> BCD436HPDriver
        self.assertEqual(get_driver_class_for_model(ScannerModel.SDS200), BCD436HPDriver)
        self.assertEqual(get_driver_class_for_model(ScannerModel.SDS100), BCD436HPDriver)
        self.assertEqual(get_driver_class_for_model(ScannerModel.BCD436HP), BCD436HPDriver)

        # XT / DMA family -> BCD996P2Driver
        self.assertEqual(get_driver_class_for_model(ScannerModel.BCD996P2), BCD996P2Driver)
        self.assertEqual(get_driver_class_for_model(ScannerModel.BCD325P2), BCD996P2Driver)
        self.assertEqual(get_driver_class_for_model(ScannerModel.BCD996XT), BCD996P2Driver)

        # Classic / DN family -> UnidenClassicDriver
        self.assertEqual(get_driver_class_for_model(ScannerModel.BC125AT), UnidenClassicDriver)
        self.assertEqual(get_driver_class_for_model(ScannerModel.BCD160DN), UnidenClassicDriver)
        self.assertEqual(get_driver_class_for_model(ScannerModel.BC780XLT), UnidenClassicDriver)

if __name__ == "__main__":
    unittest.main()
