"""Exact, bounded writer targets. Defaults retain the original 2027 output."""
PART_PROFILES = {
    2026: 'inventor-ipt-rse31-major30-metric-analytic-base-feature-v1',
    2027: 'inventor-ipt-rse31-metric-analytic-base-feature-v21',
}
ASSEMBLY_PROFILES = {
    2026: 'inventor-iam-rse31-major30-fixed-parts-v1',
    2027: 'inventor-iam-rse31-fixed-parts-v1',
}


def resolve_target(profiles, target_year=None, target_profile=None):
    if target_year is not None and (type(target_year) is not int or target_year not in profiles):
        raise ValueError('target_year must be 2026 or 2027')
    if target_profile is not None:
        if target_profile not in profiles.values():
            raise ValueError('unsupported writer target profile')
        profile_year = next(year for year, profile in profiles.items() if profile == target_profile)
        if target_year is not None and target_year != profile_year:
            raise ValueError('target_year and target_profile conflict')
        target_year = profile_year
    year = 2027 if target_year is None else target_year
    return year, profiles[year]
