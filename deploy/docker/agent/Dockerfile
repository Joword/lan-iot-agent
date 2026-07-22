# syntax=docker/dockerfile:1

FROM python:3.12-slim
WORKDIR /app
COPY pyproject.toml ./
COPY src ./src
RUN pip install --no-cache-dir .
EXPOSE 8000
ENV PYTHONUNBUFFERED=1
# Agent starts without requiring Hub / LLM to be up.
CMD ["uvicorn", "lan_iot_agent.main:app", "--host", "0.0.0.0", "--port", "8000"]
