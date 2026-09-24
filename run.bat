@echo off
title Uniden Scanner Mutual Exclusion Controller
echo Starting Uniden Dynamic Scanner Controller Engine...
echo Opening Web Control Dashboard at http://127.0.0.1:8000
start http://127.0.0.1:8000
python server.py
pause
