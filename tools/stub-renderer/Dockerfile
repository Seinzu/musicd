FROM python:3.13-slim

WORKDIR /app
COPY tools/stub-renderer/stub_renderer.py /app/stub_renderer.py

EXPOSE 9091/tcp 1900/udp

CMD ["python", "/app/stub_renderer.py"]
