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
Pre-flight validation rules. The `agones-palworld.validate` define is invoked
at the top of every template so the rules always execute during `helm install`
or `helm template`. Each rule calls `fail` on violation, which aborts the render.
*/ -}}

{{- define "agones-palworld.validate" -}}

{{- if not .Values.palworld.image.tag -}}
{{- fail "palworld.image.tag is required (format: \"vX.X.X\" or \"vX.X.X@sha256:digest\")." -}}
{{- end -}}

{{- if not .Values.sidecar.image.tag -}}
{{- fail "sidecar.image.tag is required (defaults to chart appVersion; CI fills in the digest)." -}}
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

{{- end -}}