import unittest
import os
import sys

# Add project root to sys.path
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "..")))

from discovery.serial_detector import parse_mdl_response, parse_si_response, extract_port_digits
from discovery.net_detector import parse_sus_response
from config import ScannerModel, get_allowed_baud_rates

class TestDiscovery(unittest.TestCase):

    def test_parse_mdl_response(self):
        res1 = parse_mdl_response("\rMDL,BCD436HP\r")
        self.assertEqual(res1, "BCD436HP")

        res2 = parse_mdl_response("MDL,SDS100,V1.00\r")
        self.assertEqual(res2, "SDS100")

        res3 = parse_mdl_response("ERR\r")
        self.assertIsNone(res3)

    def test_parse_si_response(self):
        res1 = parse_si_response("SI BC346XT,0,0,0\r")
        self.assertEqual(res1, "BC346XT")

        res2 = parse_si_response("INVALID\r")
        self.assertIsNone(res2)

    def test_extract_port_digits(self):
        self.assertEqual(extract_port_digits("COM1"), 1)
        self.assertEqual(extract_port_digits("COM3"), 3)
        self.assertEqual(extract_port_digits("COM10"), 10)
        self.assertEqual(extract_port_digits("COM25"), 25)

    def test_model_baud_constraints(self):
        self.assertEqual(get_allowed_baud_rates(ScannerModel.BC125AT), [115200])
        self.assertEqual(get_allowed_baud_rates(ScannerModel.BCD160DN), [115200])
        self.assertEqual(get_allowed_baud_rates(ScannerModel.BC780XLT), [19200, 9600, 4800, 2400])
        self.assertIn(115200, get_allowed_baud_rates(ScannerModel.BCD436HP))
        self.assertIn(57600, get_allowed_baud_rates(ScannerModel.BCD996P2))

    def test_parse_sus_response(self):
        raw = "SUS,UNIDEN,SCANNER,BCD536HP,Main Tower Scanner\r"
        parsed = parse_sus_response(raw, "192.168.1.50")
        self.assertIsNotNone(parsed)
        self.assertEqual(parsed.ip_address, "192.168.1.50")
        self.assertEqual(parsed.model, "BCD536HP")
        self.assertEqual(parsed.name, "Main Tower Scanner")

if __name__ == "__main__":
    unittest.main()
