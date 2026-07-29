{{- /*
Shared template helpers.
*/ -}}

{{- define "agones-palworld.fullname" -}}
{{- $name := default .Chart.Name .Values.nameOverride -}}
{{- if eq .Release.Name $name -}}
{{- $name | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- printf "%s-%s" $name .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}

{{- define "agones-palworld.labels" -}}
app.kubernetes.io/name: {{ include "agones-palworld.fullname" . | quote }}
app.kubernetes.io/instance: {{ .Release.Name | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service | quote }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" | quote }}
agones.dev/fleet: {{ include "agones-palworld.fullname" . | quote }}
{{- end -}}

{{- define "agones-palworld.selectorLabels" -}}
app.kubernetes.io/name: {{ include "agones-palworld.fullname" . | quote }}
app.kubernetes.io/instance: {{ .Release.Name | quote }}
{{- end -}}

{{- /*
Validation rules. Each one fails template render if violated.
*/ -}}

{{- if not .Values.palworld.image.tag -}}
{{- fail "palworld.image.tag is required (set to the palserver version+@sha256: digest)." -}}
{{- end -}}

{{- if and (not .Values.sidecar.image.tag) (not .Values.sidecar.image.digest) -}}
{{- fail "sidecar.image requires either .tag or .digest; CI must pin the digest." -}}
{{- end -}}

{{- if not .Values.secret.enabled -}}
{{- if not .Values.secret.existingSecret -}}
{{- fail "secret.enabled is false and secret.existingSecret is empty; cannot wire PALWORLD_ADMIN_PASSWORD." -}}
{{- end -}}
{{- end -}}

{{- if and .Values.metrics.serviceMonitor.enabled (not .Values.metrics.service.enabled) -}}
{{- fail "metrics.serviceMonitor.enabled requires metrics.service.enabled." -}}
{{- end -}}

{{- range $k, $v := .Values.palworld.env -}}
{{- if not (hasPrefix "PALWORLD_" $k) -}}
{{- fail (printf "palworld.env key %q must start with PALWORLD_ (the patch script ignores it otherwise)." $k) -}}
{{- end -}}
{{- end -}}

{{- if eq (.Values.palworld.env.PALWORLD_RESTAPI_ENABLED | default "") "False" -}}
{{- fail "palworld.env.PALWORLD_RESTAPI_ENABLED=False breaks the sidecar (no REST API to poll)." -}}
{{- end -}}
