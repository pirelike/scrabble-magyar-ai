import time
from collections import defaultdict


class RateLimiter:
    """Generikus rate limiter Socket.IO (SID) és HTTP (IP) kérésekhez."""

    _IP_PRUNE_THRESHOLD = 1000

    def __init__(self, socket_limits, ip_limits):
        """
        socket_limits: {event_name: (max_requests, window_seconds)}
        ip_limits: {action_name: (max_requests, window_seconds)}
        """
        self._socket_limits = socket_limits
        self._ip_limits = ip_limits
        self._socket_history = defaultdict(lambda: defaultdict(list))
        self._ip_history = defaultdict(lambda: defaultdict(list))
        # Futásidejű felülbírálatok (admin panel): az eredeti korlát-táblákat nem írják át
        self._socket_overrides = {}
        self._ip_overrides = {}

    def set_overrides(self, ip=None, socket=None):
        """A forgalomkorlátok felülbírálata ({név: (max, ablak_mp)}); üres / None → az eredeti értékek."""
        self._ip_overrides = {k: tuple(v) for k, v in (ip or {}).items()}
        self._socket_overrides = {k: tuple(v) for k, v in (socket or {}).items()}

    def ip_limit(self, action):
        return self._ip_overrides.get(action) or self._ip_limits.get(action)

    def socket_limit(self, event):
        return self._socket_overrides.get(event) or self._socket_limits.get(event)

    def effective_limits(self):
        """A jelenleg érvényes korlátok és az alapértelmezettek: {'http': {név: {...}}, 'socket': {...}}."""
        def table(defaults, overrides):
            return {name: {'default': list(defaults.get(name, ())) or None,
                           'current': list(overrides.get(name) or defaults.get(name) or ()) or None,
                           'overridden': name in overrides}
                    for name in sorted(set(defaults) | set(overrides))}
        return {'http': table(self._ip_limits, self._ip_overrides),
                'socket': table(self._socket_limits, self._socket_overrides)}

    def limited_ips(self, now=None):
        """A jelenleg korlátozott IP-k: [{ip, action, count, max, window, retry_in}]."""
        now = now if now is not None else time.time()
        rows = []
        for ip, actions in list(self._ip_history.items()):
            for action, stamps in list(actions.items()):
                limit = self.ip_limit(action)
                if not limit:
                    continue
                maximum, window = limit
                recent = [t for t in stamps if now - t < window]
                if len(recent) >= maximum:
                    rows.append({'ip': ip, 'action': action, 'count': len(recent), 'max': maximum,
                                 'window': window, 'retry_in': max(0, int(window - (now - min(recent))))})
        return rows

    def limited_sids(self, now=None):
        """A jelenleg korlátozott socket kapcsolatok: [{sid, event, count, max, window, retry_in}]."""
        now = now if now is not None else time.time()
        rows = []
        for sid, events in list(self._socket_history.items()):
            for event, stamps in list(events.items()):
                limit = self.socket_limit(event)
                if not limit:
                    continue
                maximum, window = limit
                recent = [t for t in stamps if now - t < window]
                if len(recent) >= maximum:
                    rows.append({'sid': sid, 'event': event, 'count': len(recent), 'max': maximum,
                                 'window': window, 'retry_in': max(0, int(window - (now - min(recent))))})
        return rows

    def clear_ip(self, ip):
        """Egy IP korlátozásának feloldása (az előzmények törlése). Visszatér: igaz, ha volt mit törölni."""
        return self._ip_history.pop(ip, None) is not None

    def check_socket(self, sid, event):
        """Socket.IO event rate limit ellenőrzés. True = engedélyezve."""
        limit = self.socket_limit(event)
        if not limit:
            return True
        max_requests, window = limit
        now = time.time()
        timestamps = self._socket_history[sid][event]
        self._socket_history[sid][event] = [t for t in timestamps if now - t < window]
        if len(self._socket_history[sid][event]) >= max_requests:
            return False
        self._socket_history[sid][event].append(now)
        return True

    def check_ip(self, ip, action):
        """IP-alapú rate limit ellenőrzés (auth endpointokra). True = engedélyezve."""
        limit = self.ip_limit(action)
        if not limit:
            return True
        max_requests, window = limit
        now = time.time()
        if len(self._ip_history) > self._IP_PRUNE_THRESHOLD:
            self._prune_ip_history(now)
        history = self._ip_history[ip]
        history[action] = [t for t in history[action] if now - t < window]
        if len(history[action]) >= max_requests:
            return False
        history[action].append(now)
        return True

    def _prune_ip_history(self, now):
        """Törli a már lejárt időbélyegű IP bejegyzéseket (memória-növekedés ellen)."""
        longest_window = max((w for _, w in list(self._ip_limits.values()) + list(self._ip_overrides.values())),
                             default=0)
        for ip in list(self._ip_history):
            actions = self._ip_history[ip]
            if not any(t for stamps in actions.values() for t in stamps
                       if now - t < longest_window):
                del self._ip_history[ip]

    def clear_sid(self, sid):
        """SID törlése disconnect-kor."""
        self._socket_history.pop(sid, None)
