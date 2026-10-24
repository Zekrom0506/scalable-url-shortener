-- wrk load script: POST /generate_url with a unique long URL per request.
-- wrk reports any failed requests on its "Non-2xx or 3xx responses" line.
wrk.method = "POST"
wrk.headers["Content-Type"] = "application/json"
wrk.headers["API-Key"] = os.getenv("API_KEY") or "123456789"

function init(args)
  math.randomseed(os.time() + math.floor(os.clock() * 1000000))
end

function request()
  local url = string.format("https://example.com/page/%d", math.random(1, 10000000))
  return wrk.format("POST", "/generate_url", nil, string.format('{"long_url":"%s"}', url))
end
