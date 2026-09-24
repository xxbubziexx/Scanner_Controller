import logging
from typing import Type
from config import ScannerConfig, ScannerModel
from drivers.base_driver import BaseScannerDriver
from drivers.bcd436hp import BCD436HPDriver
from drivers.bcd996p2 import BCD996P2Driver
from drivers.classic_driver import UnidenClassicDriver

logger = logging.getLogger("DriverFactory")

HOMEPATROL_SDS_MODELS = {
    ScannerModel.BCD436HP, ScannerModel.BCD536HP, ScannerModel.SDS100,
    ScannerModel.SDS200, ScannerModel.SDS150, ScannerModel.SDS100E,
    ScannerModel.SDS200E, ScannerModel.USDS100, ScannerModel.UBCD3600XLT,
    ScannerModel.UBCD436PT, ScannerModel.UBCD536PT
}

XT_DMA_MODELS = {
    ScannerModel.BCD996P2, ScannerModel.BCD325P2, ScannerModel.BCD996XT,
    ScannerModel.BCD396XT, ScannerModel.BCT15X, ScannerModel.BCT15,
    ScannerModel.BC346XT, ScannerModel.BC346XTC, ScannerModel.BR330T,
    ScannerModel.BCD396T, ScannerModel.BCD996T
}

CLASSIC_DN_MODELS = {
    ScannerModel.BC125AT, ScannerModel.BCD160DN, ScannerModel.BCD260DN,
    ScannerModel.UBCD160DN, ScannerModel.UBCD260DN, ScannerModel.UBC125XLT,
    ScannerModel.UBC126AT, ScannerModel.BC250D, ScannerModel.BC296D,
    ScannerModel.BC780XLT, ScannerModel.BC785D, ScannerModel.BC796D
}

def get_driver_class_for_model(model: ScannerModel) -> Type[BaseScannerDriver]:
    """Returns the driver class appropriate for any of the 34 Uniden scanner models"""
    if model in HOMEPATROL_SDS_MODELS:
        return BCD436HPDriver
    elif model in XT_DMA_MODELS:
        return BCD996P2Driver
    elif model in CLASSIC_DN_MODELS:
        return UnidenClassicDriver
    # Default fallback
    return BCD436HPDriver

def create_driver_for_model(config: ScannerConfig) -> BaseScannerDriver:
    """Factory method instantiating the appropriate driver for any Uniden model"""
    driver_cls = get_driver_class_for_model(config.model)
    logger.info(f"Instantiating {driver_cls.__name__} for {config.name} ({config.model.value})")
    return driver_cls(config)
