FROM python@sha256:528257d48c1da0dcecc2e725d1ae34498d60c965f1241e39cd6a85a8859bdf84
RUN python -m pip install --no-cache-dir setuptools==65.5.1 wheel==0.34.2
